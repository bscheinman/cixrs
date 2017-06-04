use libcix::order::trade_types::*;
use messages::EngineMessage;
use bincode::{serialize, deserialize, Bounded}; 
use memmap::{Mmap, Protection};
use std::error::Error;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct WalHeader {
    bytes_used: u64
}

enum WriteResult {
    Success,
    LogFull,
    WriteError(String)
}

struct WalFile {
    f: File,
    mem: Mmap,
    cursor: usize,
    capacity: usize
}

impl WalFile {
    fn new<P: AsRef<Path>>(path: P, size: usize) -> Result<Self, String> {
        let f = try!(OpenOptions::new().create_new(true).read(true).write(true).open(path).map_err(|e| {
            "failed to create file".to_string()
        }));
        try!(f.set_len(size as u64).map_err(|e| {
            "failed to size file".to_string()
        }));

        let mem = try!(Mmap::open(&f, Protection::ReadWrite).map_err(|e| {
            format!("failed to map file ({})", e.description())
        }));

        Ok(WalFile {
            f: f,
            mem: mem,
            cursor: 0 as usize,
            capacity: size
        })
    }

    fn write_entry(&mut self, entry: &EngineMessage) -> WriteResult {
        match serialize(entry, Bounded((self.capacity - self.cursor) as u64)) {
            Ok(bytes) => {
                {
                    let raw_bytes = unsafe { self.mem.as_mut_slice() };
                    raw_bytes[self.cursor..(self.cursor + bytes.len())].clone_from_slice(bytes.as_slice());
                }
                self.mem.flush_range(self.cursor, bytes.len());
                self.cursor += bytes.len();
                WriteResult::Success
            },
            Err(e) => {
                match e {
                    SizeLimit => WriteResult::LogFull,
                    _ => WriteResult::WriteError(e.description().to_string())
                }
            }
        }
    }
}

pub struct Wal {
    dir: PathBuf,
    index: usize,
    file_size: usize,
    // For now just use one file and rotate as needed
    // In the future we might want to have a background thread that rotates logs
    // and prepares upcoming files in advance.
    wal: WalFile
}

impl Wal {
    fn next_file<P: AsRef<Path>>(dir: P, file_size: usize, start_index: usize) ->
            Result<(WalFile, usize), String> {
        let mut index = start_index;
        while true {
            let wal_path = dir.as_ref().join(format!("wal_{}", index));
            let path_name = wal_path.to_str().unwrap_or("<unknown>").to_string();

            if wal_path.exists() {
                // XXX: If there's still room in the log, reuse it
                // This would require either maintaining the log size in the log
                // file itself, or reading through to find the first available
                // location.
                println!("wal already exists at {}", path_name);
                index += 1;
                continue;
            }

            let wal = try!(WalFile::new(wal_path, file_size).map_err(|e| {
                format!("failed to rotate wal to {}: {}", path_name, e)
            }));

            println!("opened wal at {}", path_name);

            return Ok((wal, index + 1))
        }

        unreachable!()
    }

    fn rotate(&mut self) -> Result<(), String> {
        // File and Mmap both automatically clean up when they go out of scope
        let (next_wal, next_index) = try!(Self::next_file(self.dir.as_path(), self.file_size,
                                                          self.index));

        self.wal = next_wal;
        self.index = next_index;

        Ok(())
    }

    pub fn new<P: AsRef<Path>>(dir: P, file_size: usize) -> Result<Self, String> {
        if !dir.as_ref().is_dir() {
            return Err("directory does not exist".to_string());
        }

        let mut dir_buf = PathBuf::new();
        dir_buf.push(dir);

        let (wal_file, first_index) = try!(Self::next_file(dir_buf.as_path(), file_size,
                                                           0 as usize));

        let mut wal = Wal {
            dir: dir_buf,
            index: first_index,
            file_size: file_size,
            wal: wal_file
        };

        Ok(wal)
    }

    pub fn write_entry(&mut self, entry: &EngineMessage) -> Result<(), String> {
        match self.wal.write_entry(entry) {
            WriteResult::Success => Ok(()),
            WriteResult::WriteError(s) => Err(s),
            WriteResult::LogFull => {
                try!(self.rotate());
                match self.wal.write_entry(entry) {
                    WriteResult::Success => Ok(()),
                    WriteResult::WriteError(s) => Err(s),
                    WriteResult::LogFull => Err("log files too small for entry".to_string())
                }
            }
        }
    }
}

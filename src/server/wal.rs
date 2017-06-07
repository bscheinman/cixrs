use libcix::order::trade_types::*;
use messages::EngineMessage;
use bincode::{serialize, deserialize, deserialize_from, serialized_size, Bounded}; 
use memmap::{Mmap, Protection};
use std::error::Error;
use std::fs::{File, OpenOptions, read_dir, ReadDir};
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

pub struct WalFile {
    f: File,
    mem: Mmap,
    cursor: usize,
    capacity: usize
}

impl WalFile {
    fn open_impl<P: AsRef<Path>>(path: P, size: usize, writable: bool) -> Result<Self, String> {
        let f = try!(OpenOptions::new().create_new(writable).read(true).write(writable).open(path.as_ref()).map_err(|e| {
            "failed to create file".to_string()
        }));

        let mut file_size = size;

        if writable {
            try!(f.set_len(file_size as u64).map_err(|e| {
                "failed to size file".to_string()
            }));
        } else {
            file_size = try!(f.metadata().map_err(|e| {
                "failed to read file size".to_string()
            })).len() as usize;
        }

        let prot = if writable {
            Protection::ReadWrite
        } else {
            Protection::Read
        };

        let mem = try!(Mmap::open(&f, prot).map_err(|e| {
            format!("failed to map file ({})", e.description())
        }));

        Ok(WalFile {
            f: f,
            mem: mem,
            cursor: 0 as usize,
            capacity: file_size
        })
    }

    fn create<P: AsRef<Path>>(path: P, size: usize) -> Result<Self, String> {
        Self::open_impl(path, size, true)
    }

    fn open<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        Self::open_impl(path, 0, false)
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

pub struct WalReader {
    wal: WalFile
}

impl WalReader {
    pub fn new(wal: WalFile) -> Self {
        WalReader {
            wal: wal
        }
    }

    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        Ok(Self::new(try!(WalFile::open(path))))
    }
}

impl Iterator for WalReader {
    type Item = Result<EngineMessage, String>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.wal.cursor == self.wal.capacity {
            return None;
        }

        match deserialize::<EngineMessage>(&(unsafe { self.wal.mem.as_mut_slice() }[self.wal.cursor..self.wal.capacity])) {
            Ok(ref msg) => {
                // This is a very hacky way of checking for the end of the log.
                // Really we should track in a header how far we've written or something like that
                // but this will match zeroed out memory and tell us where to stop reading.
                if let EngineMessage::NullMessage = *msg {
                    None
                } else {
                    // Is this really the best way to advance the cursor?
                    // I don't see anything in the bincode documentation that provides the byte count
                    // as part of the deserialization call
                    self.wal.cursor += serialized_size(msg) as usize;
                    //Some(Ok((*msg).clone()))
                    Some(Ok((*msg).clone()))
                }
            },
            Err(e) => {
                Some(Err(format!("invalid read at position {}: {}",
                                 self.wal.cursor, e.description())))
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
        loop {
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

            let wal = try!(WalFile::create(wal_path, file_size).map_err(|e| {
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

pub struct WalDirectoryReader {
    dir_iter: ReadDir,
    prefix: String,
    reader: Option<WalReader>
}

impl WalDirectoryReader {
    pub fn new<P: AsRef<Path>>(dir: P, prefix: String) -> Result<Self, String> {
        let path_name = dir.as_ref().to_str().unwrap_or("<unknown>").to_string();
        let dir_iter = try!(read_dir(dir).map_err(|e| {
            format!("failed to walk directory {}", path_name)
        }));

        Ok(WalDirectoryReader {
            dir_iter: dir_iter,
            prefix: prefix,
            reader: None
        })
    }
}

impl Iterator for WalDirectoryReader {
    type Item = Result<EngineMessage, String>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(ref mut reader) = self.reader {
                if let Some(msg) = reader.next() {
                    return Some(msg);
                }
            }

            let entry = match self.dir_iter.next() {
                Some(res) => {
                    match res {
                        Ok(e) => e,
                        Err(e) => {
                            return Some(Err(e.description().to_string()));
                        }
                    }
                },
                None => {
                    // Done iterating directory
                    return None;
                }
            };

            self.reader = Some(match WalReader::from_path(entry.path().as_path()) {
                Ok(r) => r,
                Err(e) => {
                    return Some(Err(e));
                }
            });
        }

        unreachable!()
    }
}

use libcix::order::trade_types::*;
use messages::EngineMessage;
use bincode::{serialize, deserialize, deserialize_from, serialized_size, Bounded}; 
use memmap::{Mmap, Protection};
use regex::Regex;
use std::error::Error;
use std::ffi::OsString;
use std::fs::{File, OpenOptions, read_dir, ReadDir};
use std::path::{Path, PathBuf};
use std::slice;
use std::str::FromStr;
use std::vec::Vec;

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
    fn open_impl<P: AsRef<Path>>(path: P, size: usize, create: bool, writable: bool)
                -> Result<Self, String> {
        let f = try!(OpenOptions::new().create_new(create).read(true).write(writable)
                     .open(path.as_ref()).map_err(|e| {
            "failed to create file".to_string()
        }));

        let mut file_size = size;

        if create {
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
        Self::open_impl(path, size, true, true)
    }

    pub fn open<P: AsRef<Path>>(path: P, writable: bool) -> Result<Self, String> {
        Self::open_impl(path, 0, false, writable)
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

    fn advance_entry(&mut self) -> Option<Result<EngineMessage, String>> {
        if self.cursor == self.capacity {
            return None;
        }

        match deserialize::<EngineMessage>(&(unsafe { self.mem.as_mut_slice() }[self.cursor..self.capacity])) {
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
                    self.cursor += serialized_size(msg) as usize;
                    //Some(Ok((*msg).clone()))
                    Some(Ok((*msg).clone()))
                }
            },
            Err(e) => {
                Some(Err(format!("invalid read at position {}: {}",
                                 self.cursor, e.description())))
            }
        }
    }

    fn advance_to_end(&mut self) -> Result<(), String> {
        self.last().map(|msg| {
            msg.map(|_| ())
        }).unwrap_or(Ok(()))
    }
}

impl Iterator for WalFile {
    type Item = Result<EngineMessage, String>;

    fn next(&mut self) -> Option<Self::Item> {
        self.advance_entry()
    }
}

pub struct Wal {
    dir: PathBuf,
    index: u32,
    file_size: usize,
    // For now just use one file and rotate as needed
    // In the future we might want to have a background thread that rotates logs
    // and prepares upcoming files in advance.
    wal: WalFile
}

impl Wal {
    fn next_file<P: AsRef<Path>>(dir: P, file_size: usize, start_index: u32) ->
            Result<(WalFile, u32), String> {
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
    }

    fn rotate(&mut self) -> Result<(), String> {
        println!("rotating wal file from {}", self.index);

        // File and Mmap both automatically clean up when they go out of scope
        let (next_wal, next_index) = try!(Self::next_file(self.dir.as_path(), self.file_size,
                                                          self.index));

        println!("rotated wal file to {}", next_index);

        self.wal = next_wal;
        self.index = next_index;

        Ok(())
    }

    pub fn new<P: AsRef<Path>>(dir: P, file_size: usize) -> Result<Self, String> {
        if !dir.as_ref().is_dir() {
            return Err("directory does not exist".to_string());
        }

        let mut dir_buf = PathBuf::new();
        dir_buf.push(dir.as_ref());

        let (wal_file, first_index) = try!(try!(Wal::get_all_files(dir.as_ref())).iter().last().map(|index| {
            println!("opening most recent wal file {}", *index);
            (Wal::open_file(dir.as_ref(), *index, true), *index)
        }).and_then(|(wal, index)| {
            match wal.map(|mut w| { w.advance_to_end(); w }) {
                Ok(w) => {
                    println!("resuming wal file {} at position {}/{}", index, w.cursor, w.capacity);
                    Some(Ok((w, index)))
                },
                Err(e) => None
            }
        }).unwrap_or_else(|| {
            println!("creating new wal file");
            Self::next_file(dir_buf.as_path(), file_size, 0u32)
        }));

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

    fn get_all_files<P: AsRef<Path>>(dir: P) -> Result<Vec<u32>, String> {
        let path_name = dir.as_ref().to_str().unwrap_or("<unknown>").to_string();
        let dir_iter: ReadDir = try!(read_dir(dir).map_err(|e| {
            format!("failed to walk directory {}", path_name)
        }));

        let wal_regex = Regex::new(r"^wal_(\d+)$").unwrap();
        let mut wal_files: Vec<u32> = dir_iter.filter_map(|item| {
            let entry = item.unwrap();
            if entry.file_type().unwrap().is_file() {
                wal_regex.captures(entry.path().file_name().unwrap().to_str().unwrap()).map(|c| {
                    u32::from_str(&c[1]).unwrap()
                })
            } else {
                None
            }
        }).collect();
        wal_files.sort();

        Ok(wal_files)
    }

    fn open_file<P: AsRef<Path>>(dir: P, index: u32, writable: bool) -> Result<WalFile, String> {
        let mut path = Path::new(dir.as_ref()).to_path_buf();
        let basename = format!("wal_{}", index);

        path.push(basename);

        WalFile::open(path.as_path(), writable)
    }
}

pub struct WalDirectoryReader {
    dir: OsString,
    files: Vec<u32>,
    file_index: usize,
    reader: Option<WalFile>
}

impl WalDirectoryReader {
    pub fn new<P: AsRef<Path>>(dir: P) -> Result<Self, String> {
        Ok(WalDirectoryReader {
            dir: dir.as_ref().as_os_str().to_os_string(),
            files: try!(Wal::get_all_files(dir)),
            file_index: 0usize,
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

            if self.file_index >= self.files.len() {
                return None;
            }

            self.reader = Some(match Wal::open_file(Path::new(&self.dir),
                                                    self.files[self.file_index], false) {
                Ok(r) => r,
                Err(e) => {
                    return Some(Err(e));
                }
            });

            self.file_index += 1;
        }

        unreachable!()
    }
}

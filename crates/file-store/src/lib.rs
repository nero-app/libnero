mod entry;
mod error;

pub use error::Error;

use std::{
    collections::HashMap,
    fmt::Write,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};

use crate::entry::FileEntry;

type FileLock = Arc<Mutex<()>>;

pub struct FileStore {
    root: PathBuf,
    file_locks: Mutex<HashMap<PathBuf, FileLock>>,
    size_bytes: Mutex<u64>,
    max_bytes: Option<u64>,
}

impl FileStore {
    pub fn new(root: PathBuf, max_bytes: Option<u64>) -> Result<Self, Error> {
        fs::create_dir_all(&root)?;
        let initial = Self::scan_bytes(root.clone());

        Ok(Self {
            root,
            file_locks: Mutex::new(HashMap::new()),
            size_bytes: Mutex::new(initial),
            max_bytes,
        })
    }

    fn scan_bytes(root: PathBuf) -> u64 {
        let mut total = 0u64;
        let mut stack = vec![root];

        while let Some(dir) = stack.pop() {
            let Ok(rd) = fs::read_dir(&dir) else { continue };
            for entry in rd.flatten() {
                if entry.file_name().to_string_lossy().ends_with("~tmp") {
                    continue;
                }
                let Ok(meta) = entry.metadata() else { continue };
                if meta.is_dir() {
                    stack.push(entry.path());
                } else {
                    total += meta.len();
                }
            }
        }

        total
    }

    fn key_to_path(&self, key: &str) -> PathBuf {
        let hash = Sha256::digest(key.as_bytes());
        let mut hex = String::with_capacity(64);
        for b in &hash {
            write!(hex, "{:02x}", b).unwrap();
        }
        let (prefix, tail) = hex.split_at(2);
        self.root.join(prefix).join(tail)
    }

    fn lock_for(&self, path: &Path) -> FileLock {
        let mut map = self.file_locks.lock().unwrap();
        map.retain(|_, v| Arc::strong_count(v) > 1);
        map.entry(path.to_path_buf())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        let path = self.key_to_path(key);
        let file_lock = self.lock_for(&path);
        let _guard = file_lock.lock().unwrap();

        let data = match fs::read(&path) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        let entry = FileEntry::try_from(data.as_slice())?;

        if entry.is_expired() {
            let _ = fs::remove_file(&path);
            let mut sz = self.size_bytes.lock().unwrap();
            *sz = sz.saturating_sub(data.len() as u64);
            return Ok(None);
        }

        Ok(Some(entry.value))
    }

    pub fn set(&self, key: &str, value: Vec<u8>, ttl_ms: Option<u32>) -> Result<(), Error> {
        let path = self.key_to_path(key);
        let file_lock = self.lock_for(&path);
        let _guard = file_lock.lock().unwrap();

        let old_bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

        let entry = match ttl_ms {
            Some(ttl) => FileEntry::new(key.to_owned(), value).with_ttl(ttl),
            None => FileEntry::new(key.to_owned(), value),
        };
        let buf = entry.encode();
        let new_bytes = buf.len() as u64;

        let mut sz = self.size_bytes.lock().unwrap();
        let projected = sz.saturating_sub(old_bytes) + new_bytes;
        if self.max_bytes.is_some_and(|max| projected > max) {
            return Err(Error::StorageLimitExceeded);
        }

        let parent = path.parent().unwrap();
        fs::create_dir_all(parent)?;
        let tmp = path.with_extension("~tmp");
        fs::write(&tmp, &buf)?;
        fs::rename(&tmp, &path)?;

        *sz = projected;
        Ok(())
    }

    pub fn delete(&self, key: &str) -> Result<(), Error> {
        let path = self.key_to_path(key);
        let file_lock = self.lock_for(&path);
        let _guard = file_lock.lock().unwrap();

        let old_bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if old_bytes == 0 {
            return Ok(());
        }

        match fs::remove_file(&path) {
            Ok(()) => {
                let mut sz = self.size_bytes.lock().unwrap();
                *sz = sz.saturating_sub(old_bytes);
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::from(e)),
        }
    }

    pub fn exists(&self, key: &str) -> Result<bool, Error> {
        let path = self.key_to_path(key);
        let file_lock = self.lock_for(&path);
        let _guard = file_lock.lock().unwrap();

        let mut file = match fs::File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e.into()),
        };

        let mut header = [0u8; entry::HEADER_LEN];
        file.read_exact(&mut header)?;

        let expiry_ms = u64::from_le_bytes(header[..entry::EXPIRY_LEN].try_into().unwrap());
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let expired = expiry_ms != 0 && expiry_ms <= now_ms;

        Ok(!expired)
    }

    pub fn list_keys(&self, cursor: Option<&str>) -> Result<(Vec<String>, Option<String>), Error> {
        const PAGE_SIZE: usize = 100;

        let mut keys = Vec::new();
        let mut stack = vec![self.root.clone()];

        while let Some(dir) = stack.pop() {
            let Ok(rd) = fs::read_dir(&dir) else { continue };
            for entry in rd.flatten() {
                let Ok(ft) = entry.file_type() else { continue };
                if ft.is_dir() {
                    stack.push(entry.path());
                    continue;
                }
                if entry.file_name().to_string_lossy().ends_with("~tmp") {
                    continue;
                }
                let Ok(data) = fs::read(entry.path()) else {
                    continue;
                };
                let Ok(fe) = FileEntry::try_from(data.as_slice()) else {
                    continue;
                };
                if !fe.is_expired() {
                    keys.push(fe.key);
                }
            }
        }

        keys.sort_unstable();

        let start = cursor.map_or(0, |c| keys.partition_point(|k| k.as_str() <= c));
        let total = keys.len();
        let page = keys[start..]
            .iter()
            .take(PAGE_SIZE)
            .cloned()
            .collect::<Vec<_>>();
        let next_cursor = (start + PAGE_SIZE < total)
            .then(|| page.last().cloned())
            .flatten();

        Ok((page, next_cursor))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::tempdir;

    use crate::{Error, FileStore};

    fn store(max_bytes: Option<u64>) -> (FileStore, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let store = FileStore::new(dir.path().to_path_buf(), max_bytes).unwrap();
        (store, dir)
    }

    #[test]
    fn set_and_get() {
        let (store, _dir) = store(None);
        store.set("hello", b"world".to_vec(), None).unwrap();
        assert_eq!(store.get("hello").unwrap(), Some(b"world".to_vec()));
    }

    #[test]
    fn get_missing_returns_none() {
        let (store, _dir) = store(None);
        assert_eq!(store.get("missing").unwrap(), None);
    }

    #[test]
    fn delete_removes_key() {
        let (store, _dir) = store(None);
        store.set("k", b"v".to_vec(), None).unwrap();
        store.delete("k").unwrap();
        assert_eq!(store.get("k").unwrap(), None);
    }

    #[test]
    fn delete_missing_key_is_noop() {
        let (store, _dir) = store(None);
        assert!(store.delete("missing").is_ok());
    }

    #[test]
    fn ttl_expiry() {
        let (store, _dir) = store(None);
        store.set("k", b"v".to_vec(), Some(1)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert_eq!(store.get("k").unwrap(), None);
    }

    #[test]
    fn no_ttl_does_not_expire() {
        let (store, _dir) = store(None);
        store.set("k", b"v".to_vec(), None).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert_eq!(store.get("k").unwrap(), Some(b"v".to_vec()));
    }

    #[test]
    fn max_bytes_enforced() {
        let (store, _dir) = store(Some(20));
        store.set("k1", b"v".to_vec(), None).unwrap();
        let err = store.set("k2", b"v".to_vec(), None).unwrap_err();
        assert!(matches!(err, Error::StorageLimitExceeded));
    }

    #[test]
    fn overwrite_same_key_does_not_accumulate_size() {
        let (store, _dir) = store(Some(1024));
        store.set("k", b"aaa".to_vec(), None).unwrap();
        store.set("k", b"bbb".to_vec(), None).unwrap();
        let sz = *store.size_bytes.lock().unwrap();
        assert!(sz < 1024);
    }

    #[test]
    fn list_keys_returns_sorted_and_excludes_expired() {
        let (store, _dir) = store(None);
        store.set("alpha", b"1".to_vec(), None).unwrap();
        store.set("beta", b"2".to_vec(), None).unwrap();
        store.set("gamma", b"3".to_vec(), None).unwrap();
        store.set("dead", b"4".to_vec(), Some(1)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));

        let (keys, cursor) = store.list_keys(None).unwrap();
        assert_eq!(keys, vec!["alpha", "beta", "gamma"]);
        assert!(cursor.is_none());
    }

    #[test]
    fn concurrent_writes_respect_size_limit() {
        let dir = tempdir().unwrap();
        let store = Arc::new(FileStore::new(dir.path().to_path_buf(), Some(100)).unwrap());

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let store = Arc::clone(&store);
                std::thread::spawn(move || store.set(&format!("key{i}"), vec![0u8; 20], None))
            })
            .collect();

        let limit_errors = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(|r| matches!(r, Err(Error::StorageLimitExceeded)))
            .count();

        let sz = *store.size_bytes.lock().unwrap();
        assert!(sz <= 100, "size {sz} exceeded the limit");
        assert!(
            limit_errors > 0,
            "no writes were rejected, limit was not enforced"
        );
    }
}

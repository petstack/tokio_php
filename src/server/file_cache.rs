//! LRU cache for file system metadata.
//!
//! Caches file existence and type (file vs directory) to reduce syscalls.
//! Thread-safe with RwLock, uses LRU eviction when at capacity.

use std::collections::HashMap;
use std::path::Path;
use std::sync::RwLock;

/// Maximum number of cached entries.
const FILE_CACHE_CAPACITY: usize = 200;

/// Cached file type information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    /// Regular file
    File,
    /// Directory
    Dir,
}

/// LRU cache for file metadata.
///
/// Stores file existence and type (file/directory/none) to avoid repeated stat() calls.
/// Thread-safe with RwLock, evicts least recently used entries when at capacity.
pub struct FileCache {
    /// Cached entries: path -> file type (None = doesn't exist)
    entries: RwLock<HashMap<Box<str>, Option<FileType>>>,
    /// LRU order: most recently used at back
    order: RwLock<Vec<Box<str>>>,
    /// Maximum capacity
    capacity: usize,
}

impl FileCache {
    /// Create a new empty cache with default capacity (200 entries).
    pub fn new() -> Self {
        Self::with_capacity(FILE_CACHE_CAPACITY)
    }

    /// Create a new empty cache with specified capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: RwLock::new(HashMap::with_capacity(capacity)),
            order: RwLock::new(Vec::with_capacity(capacity)),
            capacity,
        }
    }

    /// Check file type from cache only (no filesystem access).
    /// Returns None if not in cache.
    #[inline]
    pub fn get(&self, path: &str) -> Option<Option<FileType>> {
        let entries = self.entries.read().unwrap();
        entries.get(path).copied()
    }

    /// Check file type, using cache or falling back to filesystem.
    /// Returns (file_type, cache_hit).
    ///
    /// - `Some(FileType::File)` - path is a regular file
    /// - `Some(FileType::Dir)` - path is a directory
    /// - `None` - path doesn't exist
    #[inline]
    pub fn check(&self, path: &str) -> (Option<FileType>, bool) {
        // Fast path: check cache (read lock)
        {
            let entries = self.entries.read().unwrap();
            if let Some(&file_type) = entries.get(path) {
                // Update LRU order
                self.touch(path);
                return (file_type, true);
            }
        }

        // Cache miss: check filesystem
        let file_type = Self::stat(path);

        // Insert into cache
        self.insert(path, file_type);

        (file_type, false)
    }

    /// Check if path exists and is a regular file.
    #[inline]
    pub fn is_file(&self, path: &str) -> bool {
        matches!(self.check(path).0, Some(FileType::File))
    }

    /// Check if path exists and is a directory.
    #[inline]
    pub fn is_dir(&self, path: &str) -> bool {
        matches!(self.check(path).0, Some(FileType::Dir))
    }

    /// Check if path exists (file or directory).
    #[inline]
    pub fn exists(&self, path: &str) -> bool {
        self.check(path).0.is_some()
    }

    /// Insert a path into the cache.
    fn insert(&self, path: &str, file_type: Option<FileType>) {
        let mut entries = self.entries.write().unwrap();
        let mut order = self.order.write().unwrap();

        // Check if already exists (may have been added by another thread)
        if entries.contains_key(path) {
            return;
        }

        // Evict oldest if at capacity
        if order.len() >= self.capacity {
            if let Some(oldest) = order.first().cloned() {
                entries.remove(&oldest);
                order.remove(0);
            }
        }

        // Add new entry
        let key: Box<str> = path.into();
        entries.insert(key.clone(), file_type);
        order.push(key);
    }

    /// Move path to end of LRU order (mark as recently used).
    fn touch(&self, path: &str) {
        let mut order = self.order.write().unwrap();
        if let Some(pos) = order.iter().position(|p| p.as_ref() == path) {
            let key = order.remove(pos);
            order.push(key);
        }
    }

    /// Stat a path and return its type.
    #[inline]
    fn stat(path: &str) -> Option<FileType> {
        let p = Path::new(path);
        if p.is_file() {
            Some(FileType::File)
        } else if p.is_dir() {
            Some(FileType::Dir)
        } else {
            None
        }
    }
}

impl Default for FileCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use tempfile::TempDir;

    fn setup_test_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        File::create(dir.path().join("test.txt")).unwrap();
        fs::create_dir(dir.path().join("subdir")).unwrap();
        dir
    }

    #[test]
    fn test_file_detection() {
        let dir = setup_test_dir();
        let cache = FileCache::new();

        let file_path = dir.path().join("test.txt");
        let (file_type, hit) = cache.check(file_path.to_str().unwrap());

        assert_eq!(file_type, Some(FileType::File));
        assert!(!hit); // First access = cache miss
    }

    #[test]
    fn test_dir_detection() {
        let dir = setup_test_dir();
        let cache = FileCache::new();

        let dir_path = dir.path().join("subdir");
        let (file_type, hit) = cache.check(dir_path.to_str().unwrap());

        assert_eq!(file_type, Some(FileType::Dir));
        assert!(!hit);
    }

    #[test]
    fn test_missing_file() {
        let dir = setup_test_dir();
        let cache = FileCache::new();

        let missing_path = dir.path().join("nonexistent");
        let (file_type, hit) = cache.check(missing_path.to_str().unwrap());

        assert_eq!(file_type, None);
        assert!(!hit);
    }

    #[test]
    fn test_cache_hit() {
        let dir = setup_test_dir();
        let cache = FileCache::new();

        let file_path = dir.path().join("test.txt");
        let path_str = file_path.to_str().unwrap();

        // First access
        let (_, hit1) = cache.check(path_str);
        assert!(!hit1);

        // Second access
        let (file_type, hit2) = cache.check(path_str);
        assert!(hit2);
        assert_eq!(file_type, Some(FileType::File));
    }

    #[test]
    fn test_lru_eviction() {
        let dir = setup_test_dir();
        let cache = FileCache::with_capacity(2);

        let file1 = dir.path().join("test.txt");
        let subdir = dir.path().join("subdir");

        // Fill cache
        cache.check(file1.to_str().unwrap());
        cache.check(subdir.to_str().unwrap());

        // Add third entry (should evict first)
        let missing = dir.path().join("missing");
        cache.check(missing.to_str().unwrap());

        // First entry should be evicted
        assert!(cache.get(file1.to_str().unwrap()).is_none());
        // Second and third should be in cache
        assert!(cache.get(subdir.to_str().unwrap()).is_some());
        assert!(cache.get(missing.to_str().unwrap()).is_some());
    }

    #[test]
    fn test_is_file_is_dir() {
        let dir = setup_test_dir();
        let cache = FileCache::new();

        let file_path = dir.path().join("test.txt");
        let dir_path = dir.path().join("subdir");

        assert!(cache.is_file(file_path.to_str().unwrap()));
        assert!(!cache.is_dir(file_path.to_str().unwrap()));

        assert!(cache.is_dir(dir_path.to_str().unwrap()));
        assert!(!cache.is_file(dir_path.to_str().unwrap()));
    }

    #[test]
    fn test_negative_cache() {
        let dir = setup_test_dir();
        let cache = FileCache::new();

        let missing = dir.path().join("missing");
        let path_str = missing.to_str().unwrap();

        // First check - miss, caches None
        let (file_type1, hit1) = cache.check(path_str);
        assert_eq!(file_type1, None);
        assert!(!hit1);

        // Second check - hit, returns cached None
        let (file_type2, hit2) = cache.check(path_str);
        assert_eq!(file_type2, None);
        assert!(hit2);
    }
}

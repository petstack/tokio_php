//! PHP executor using embed SAPI with SAPI name override for OPcache compatibility.

use async_trait::async_trait;
use std::ffi::{c_char, c_int, CString};
use std::ptr;

use super::common::{self, WorkerPool};
use super::{ExecutorError, ScriptExecutor};
use crate::types::{ScriptRequest, ScriptResponse};

// =============================================================================
// PHP Embed FFI Bindings (specific to this executor)
// =============================================================================

#[link(name = "php")]
extern "C" {
    fn php_embed_init(argc: c_int, argv: *mut *mut c_char) -> c_int;
    fn php_embed_shutdown();

    // php_embed_module is the embed SAPI module used by php_embed_init
    static mut php_embed_module: SapiModuleStub;
}

#[repr(C)]
struct SapiModuleStub {
    name: *mut c_char,
    pretty_name: *mut c_char,
}

/// SAPI name for OPcache/JIT compatibility
static SAPI_NAME_CLI_SERVER: &[u8] = b"cli-server\0";

// =============================================================================
// PHP Thread Pool with Embed SAPI
// =============================================================================

struct PhpThreadPool {
    pool: WorkerPool,
}

impl PhpThreadPool {
    fn new(num_workers: usize) -> Result<Self, String> {
        // Initialize PHP using embed SAPI with name override
        unsafe {
            // Override SAPI name BEFORE initialization for OPcache compatibility
            php_embed_module.name = SAPI_NAME_CLI_SERVER.as_ptr() as *mut c_char;

            let program_name = CString::new("tokio_php").unwrap();
            let mut argv: [*mut c_char; 2] = [program_name.as_ptr() as *mut c_char, ptr::null_mut()];

            let result = php_embed_init(1, argv.as_mut_ptr());
            if result != 0 {
                return Err(format!("Failed to initialize PHP embed: {}", result));
            }

            tracing::info!("PHP initialized with SAPI 'cli-server' (OPcache compatible)");
        }

        let pool = WorkerPool::new(num_workers, "php-worker", |id, rx| {
            common::worker_main_loop(id, rx);
        })?;

        for id in 0..num_workers {
            tracing::info!("Spawned PHP worker thread {}", id);
        }

        Ok(Self { pool })
    }

    async fn execute_request(&self, request: ScriptRequest) -> Result<ScriptResponse, String> {
        self.pool.execute(request).await
    }

    fn worker_count(&self) -> usize {
        self.pool.worker_count()
    }
}

impl Drop for PhpThreadPool {
    fn drop(&mut self) {
        self.pool.join_all();

        unsafe {
            php_embed_shutdown();
        }
        tracing::info!("PHP shutdown complete");
    }
}

// =============================================================================
// Public Executor Interface
// =============================================================================

/// PHP script executor using embed SAPI with SAPI name override for OPcache.
pub struct PhpExecutor {
    pool: PhpThreadPool,
}

impl PhpExecutor {
    /// Creates a new PHP executor with the specified number of worker threads.
    pub fn new(num_workers: usize) -> Result<Self, ExecutorError> {
        let pool = PhpThreadPool::new(num_workers)?;
        Ok(Self { pool })
    }

    /// Returns the number of worker threads.
    pub fn worker_count(&self) -> usize {
        self.pool.worker_count()
    }
}

#[async_trait]
impl ScriptExecutor for PhpExecutor {
    async fn execute(&self, request: ScriptRequest) -> Result<ScriptResponse, ExecutorError> {
        self.pool
            .execute_request(request)
            .await
            .map_err(ExecutorError::from)
    }

    fn name(&self) -> &'static str {
        "php-zts"
    }

    fn shutdown(&self) {
        // Pool shutdown handled by Drop
    }
}

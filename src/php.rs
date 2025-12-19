use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use std::ffi::CString;
use std::io::{Read, Write};
use std::os::raw::{c_char, c_int, c_void};
use std::os::unix::io::{FromRawFd, RawFd};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;

type size_t = usize;

#[link(name = "php")]
extern "C" {
    fn php_embed_init(argc: c_int, argv: *mut *mut c_char) -> c_int;
    fn php_embed_shutdown();

    fn php_execute_simple_script(handle: *mut ZendFileHandle, retval: *mut c_void) -> c_int;
    fn zend_stream_init_filename(handle: *mut ZendFileHandle, filename: *const c_char);
    fn zend_destroy_file_handle(handle: *mut ZendFileHandle);
}

#[repr(C)]
pub struct ZendFileHandle {
    _data: [u8; 128],
}

static PHP_INITIALIZED: AtomicBool = AtomicBool::new(false);
static PHP_EXECUTOR: OnceCell<PhpExecutor> = OnceCell::new();

enum PhpCommand {
    ExecuteFile(String, mpsc::Sender<Result<String, String>>),
    Shutdown,
}

struct PhpExecutor {
    sender: mpsc::Sender<PhpCommand>,
}

impl PhpExecutor {
    fn new() -> Result<Self, String> {
        let (tx, rx) = mpsc::channel::<PhpCommand>();

        // Spawn PHP thread with larger stack (16MB)
        thread::Builder::new()
            .name("php-executor".to_string())
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                Self::php_thread(rx);
            })
            .map_err(|e| format!("Failed to spawn PHP thread: {}", e))?;

        Ok(PhpExecutor { sender: tx })
    }

    fn php_thread(rx: mpsc::Receiver<PhpCommand>) {
        // Initialize PHP
        unsafe {
            let program_name = CString::new("tokio_php").unwrap();
            let mut argv: [*mut c_char; 2] = [program_name.as_ptr() as *mut c_char, ptr::null_mut()];

            let result = php_embed_init(1, argv.as_mut_ptr());
            if result != 0 {
                tracing::error!("Failed to initialize PHP embed: {}", result);
                return;
            }
        }

        PHP_INITIALIZED.store(true, Ordering::SeqCst);
        tracing::info!("PHP runtime initialized");

        // Process commands
        while let Ok(cmd) = rx.recv() {
            match cmd {
                PhpCommand::ExecuteFile(path, response_tx) => {
                    let result = Self::do_execute_file(&path);
                    let _ = response_tx.send(result);
                }
                PhpCommand::Shutdown => {
                    break;
                }
            }
        }

        // Shutdown PHP
        unsafe {
            php_embed_shutdown();
        }
        PHP_INITIALIZED.store(false, Ordering::SeqCst);
        tracing::info!("PHP runtime shut down");
    }

    fn do_execute_file(file_path: &str) -> Result<String, String> {
        // Create a pipe to capture stdout
        let mut pipe_fds: [c_int; 2] = [0, 0];
        if unsafe { libc::pipe(pipe_fds.as_mut_ptr()) } != 0 {
            return Err("Failed to create pipe".to_string());
        }

        let read_fd = pipe_fds[0];
        let write_fd = pipe_fds[1];

        // Save original stdout
        let original_stdout = unsafe { libc::dup(1) };
        if original_stdout < 0 {
            unsafe {
                libc::close(read_fd);
                libc::close(write_fd);
            }
            return Err("Failed to dup stdout".to_string());
        }

        // Redirect stdout to our pipe
        if unsafe { libc::dup2(write_fd, 1) } < 0 {
            unsafe {
                libc::close(read_fd);
                libc::close(write_fd);
                libc::close(original_stdout);
            }
            return Err("Failed to redirect stdout".to_string());
        }

        // Execute PHP script
        unsafe {
            let path_c = CString::new(file_path).map_err(|e| e.to_string())?;

            let mut file_handle: ZendFileHandle = std::mem::zeroed();
            zend_stream_init_filename(&mut file_handle, path_c.as_ptr());

            php_execute_simple_script(&mut file_handle, ptr::null_mut());

            zend_destroy_file_handle(&mut file_handle);
        }

        // Flush stdout
        unsafe {
            libc::fflush(ptr::null_mut());
        }

        // Close write end of pipe
        unsafe {
            libc::close(write_fd);
        }

        // Restore original stdout
        unsafe {
            libc::dup2(original_stdout, 1);
            libc::close(original_stdout);
        }

        // Read output from pipe
        let mut output = String::new();
        let mut read_file = unsafe { std::fs::File::from_raw_fd(read_fd) };
        read_file.read_to_string(&mut output).map_err(|e| e.to_string())?;

        if output.is_empty() {
            return Err("PHP script produced no output".to_string());
        }

        Ok(output)
    }

    fn execute_file(&self, path: String) -> Result<String, String> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(PhpCommand::ExecuteFile(path, tx))
            .map_err(|e| format!("Failed to send command: {}", e))?;

        rx.recv().map_err(|e| format!("Failed to receive response: {}", e))?
    }

    fn shutdown(&self) {
        let _ = self.sender.send(PhpCommand::Shutdown);
    }
}

pub struct PhpRuntime;

impl PhpRuntime {
    pub fn init() -> Result<(), String> {
        PHP_EXECUTOR
            .get_or_try_init(|| PhpExecutor::new())
            .map(|_| ())
    }

    pub fn shutdown() {
        if let Some(executor) = PHP_EXECUTOR.get() {
            executor.shutdown();
        }
    }

    pub fn execute_file(file_path: &str) -> Result<String, String> {
        let executor = PHP_EXECUTOR
            .get()
            .ok_or("PHP runtime not initialized")?;

        executor.execute_file(file_path.to_string())
    }
}

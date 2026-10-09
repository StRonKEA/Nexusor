mod desktop;
#[cfg(not(dev))]
mod frontend;
mod startup;
mod tray;

#[global_allocator]
static GLOBAL_ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub fn run() -> std::process::ExitCode {
    desktop::run()
}

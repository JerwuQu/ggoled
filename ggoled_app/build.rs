use std::io;

fn main() -> io::Result<()> {
    #[cfg(target_os = "windows")]
    winresource::WindowsResource::new()
        .set_icon("assets/ggoled.ico")
        .compile()?;
    Ok(())
}

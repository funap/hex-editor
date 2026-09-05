fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icons/xvw.ico");
        if let Err(e) = res.compile() {
            eprintln!("Failed to compile Windows resources: {e}");
        }
    }
}

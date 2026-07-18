fn main() {
    let mut res = winres::WindowsResource::new();
    res.set_language(0x0409 /* English */);
    res.set("FileDescription", env!("CARGO_PKG_DESCRIPTION"));
    res.set("ProductName", "BurntSushi");
    res.set("OriginalFilename", "BurntSushiBlocker_x64.dll");
    res.set("CompanyName", "OpenByte");
    res.compile().unwrap();
}

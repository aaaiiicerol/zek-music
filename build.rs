// Coloca o ícone e as informações de versão dentro do zek-music.exe (só no Windows).
fn main() {
    println!("cargo:rerun-if-changed=packaging/zek-music.ico");
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("packaging/zek-music.ico");
        res.set("ProductName", "Zek Music");
        res.set("FileDescription", "Zek Music - player de músicas");
        res.set("CompanyName", "aaaiiicerol");
        res.set("LegalCopyright", "MIT License");
        if let Err(e) = res.compile() {
            eprintln!("aviso: não foi possível embutir o ícone: {e}");
        }
    }
}

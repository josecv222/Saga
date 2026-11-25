//! Instalador de Saga: un solo .exe que lleva la app dentro.
//! Instala para el usuario actual (sin permisos de administrador) en
//! %LOCALAPPDATA%\Programs\Saga, con accesos en Inicio y escritorio,
//! «Abrir con Saga» para los formatos de libro y su entrada en
//! Configuración > Aplicaciones para desinstalar.
//!
//! `--uninstall` desinstala. `--quiet` no pregunta (pruebas).
//! `SAGA_SETUP_ROOT` lo desvía todo a otra carpeta y otra clave del registro (pruebas).
#![windows_subsystem = "windows"]

use std::fs;
use std::path::{Path, PathBuf};
use windows::core::{w, Interface, BOOL, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Shell::*;

static APP: &[u8] = include_bytes!(env!("SAGA_EXE"));
const VERSION: &str = env!("CARGO_PKG_VERSION");
const EXTS: &[&str] = &["epub", "pdf", "mobi", "azw", "azw3", "fb2", "cbz", "cbr", "cb7", "djvu", "djv", "docx", "odt", "rtf", "txt", "md", "html", "htm", "xhtml"];
const PROGID: &str = "Saga.Libro";

struct Places {
    dir: PathBuf,
    menu: PathBuf,
    desktop: PathBuf,
    /// "Software" o, en pruebas, "Software\SagaPrueba".
    soft: String,
}

fn known(id: &windows::core::GUID) -> PathBuf {
    unsafe {
        match SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) {
            Ok(p) => {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as _));
                PathBuf::from(s)
            }
            Err(_) => PathBuf::new(),
        }
    }
}

fn places() -> Places {
    if let Ok(root) = std::env::var("SAGA_SETUP_ROOT") {
        let r = PathBuf::from(root);
        return Places { dir: r.join("Programs").join("Saga"), menu: r.join("Menu"), desktop: r.join("Escritorio"), soft: "Software\\SagaPrueba".into() };
    }
    Places {
        dir: known(&FOLDERID_LocalAppData).join("Programs").join("Saga"),
        menu: known(&FOLDERID_Programs),
        desktop: known(&FOLDERID_Desktop),
        soft: "Software".into(),
    }
}

// ------------------------------------------------------------ diálogos

/// Diálogo de Windows con botones grandes. Devuelve (botón, casilla).
fn ask(main: &str, content: &str, buttons: &[(i32, &str)], check: Option<(&str, bool)>, warn: bool) -> (i32, bool) {
    if quiet() {
        return (buttons.first().map_or(1, |b| b.0), check.is_some_and(|c| c.1) || std::env::var("SAGA_SETUP_CHECK").is_ok());
    }
    let title = HSTRING::from("Saga");
    let m = HSTRING::from(main);
    let c = HSTRING::from(content);
    let texts: Vec<HSTRING> = buttons.iter().map(|b| HSTRING::from(b.1)).collect();
    let bs: Vec<TASKDIALOG_BUTTON> = buttons.iter().zip(&texts).map(|(b, t)| TASKDIALOG_BUTTON { nButtonID: b.0, pszButtonText: PCWSTR(t.as_ptr()) }).collect();
    let vt = HSTRING::from(check.map_or("", |c| c.0));
    let mut flags = TDF_ALLOW_DIALOG_CANCELLATION | TDF_USE_COMMAND_LINKS | TDF_POSITION_RELATIVE_TO_WINDOW;
    if check.is_some_and(|c| c.1) {
        flags |= TDF_VERIFICATION_FLAG_CHECKED;
    }
    let mut cfg = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hInstance: unsafe { GetModuleHandleW(None).map(|h| HINSTANCE(h.0)).unwrap_or_default() },
        dwFlags: flags,
        dwCommonButtons: TDCBF_CANCEL_BUTTON,
        pszWindowTitle: PCWSTR(title.as_ptr()),
        pszMainInstruction: PCWSTR(m.as_ptr()),
        pszContent: PCWSTR(c.as_ptr()),
        cButtons: bs.len() as u32,
        pButtons: bs.as_ptr(),
        nDefaultButton: buttons.first().map_or(0, |b| b.0),
        ..Default::default()
    };
    // el icono de Saga (recurso 1) o el de aviso
    cfg.Anonymous1.pszMainIcon = if warn { TD_WARNING_ICON } else { PCWSTR(1 as _) };
    if check.is_some() {
        cfg.pszVerificationText = PCWSTR(vt.as_ptr());
    }
    let mut pressed = 0;
    let mut checked = BOOL(0);
    unsafe {
        let _ = TaskDialogIndirect(&cfg, Some(&mut pressed), None, Some(&mut checked));
    }
    (pressed, checked.as_bool())
}

fn tell(main: &str, content: &str, warn: bool) {
    if quiet() {
        eprintln!("{main}: {content}");
        return;
    }
    let _ = ask(main, content, &[(1, "Aceptar")], None, warn);
}

fn quiet() -> bool {
    std::env::args().any(|a| a == "--quiet")
}

// ------------------------------------------------------------ registro

fn reg_key(path: &str) -> Option<HKEY> {
    let mut hk = HKEY::default();
    let r = unsafe { RegCreateKeyExW(HKEY_CURRENT_USER, &HSTRING::from(path), None, None, REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut hk, None) };
    (r == ERROR_SUCCESS).then_some(hk)
}
fn reg_str(path: &str, name: &str, val: &str) {
    if let Some(hk) = reg_key(path) {
        let w: Vec<u16> = val.encode_utf16().chain([0]).collect();
        let b = unsafe { std::slice::from_raw_parts(w.as_ptr() as *const u8, w.len() * 2) };
        unsafe {
            let _ = RegSetValueExW(hk, &HSTRING::from(name), None, REG_SZ, Some(b));
            let _ = RegCloseKey(hk);
        }
    }
}
fn reg_dword(path: &str, name: &str, val: u32) {
    if let Some(hk) = reg_key(path) {
        unsafe {
            let _ = RegSetValueExW(hk, &HSTRING::from(name), None, REG_DWORD, Some(&val.to_le_bytes()));
            let _ = RegCloseKey(hk);
        }
    }
}
fn reg_tree_del(path: &str) {
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(path));
        let _ = RegDeleteKeyW(HKEY_CURRENT_USER, &HSTRING::from(path));
    }
}
fn reg_value_del(path: &str, name: &str) {
    let mut hk = HKEY::default();
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(path), None, KEY_SET_VALUE, &mut hk) == ERROR_SUCCESS {
            let _ = RegDeleteValueW(hk, &HSTRING::from(name));
            let _ = RegCloseKey(hk);
        }
    }
}

// ------------------------------------------------------------ accesos directos

fn shortcut(lnk: &Path, exe: &Path, dir: &Path) -> windows::core::Result<()> {
    unsafe {
        let l: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        l.SetPath(&HSTRING::from(exe.as_os_str()))?;
        l.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()))?;
        l.SetDescription(w!("Biblioteca y lector de libros"))?;
        l.SetIconLocation(&HSTRING::from(exe.as_os_str()), 0)?;
        let f: IPersistFile = l.cast()?;
        f.Save(&HSTRING::from(lnk.as_os_str()), true)?;
    }
    Ok(())
}

// ------------------------------------------------------------ instalar

fn uninstall_key(p: &Places) -> String {
    format!("{}\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Saga", p.soft)
}

/// Escribe Saga.exe; si está abierta, pide cerrarla.
fn write_app(exe: &Path) -> bool {
    loop {
        let tmp = exe.with_extension("nuevo");
        let ok = fs::write(&tmp, APP).is_ok() && {
            let _ = fs::remove_file(exe.with_extension("viejo"));
            // un .exe en uso no se puede pisar, pero sí apartar
            if exe.exists() && fs::remove_file(exe).is_err() {
                if fs::rename(exe, exe.with_extension("viejo")).is_err() {
                    false
                } else {
                    fs::rename(&tmp, exe).is_ok()
                }
            } else {
                fs::rename(&tmp, exe).is_ok()
            }
        };
        if ok {
            return true;
        }
        let _ = fs::remove_file(&tmp);
        let (b, _) = ask("Saga está abierta", "Ciérrala y pulsa Reintentar.", &[(100, "Reintentar")], None, true);
        if b != 100 {
            return false;
        }
    }
}

fn install() {
    let p = places();
    let exe = p.dir.join("Saga.exe");
    let again = exe.exists();
    let (b, desk) = ask(
        if again { "Actualizar Saga" } else { "Instalar Saga" },
        &format!(
            "Biblioteca y lector de libros, versión {VERSION}.\n\nSe instalará solo para tu usuario, sin permisos de administrador, en:\n{}{}",
            p.dir.display(),
            if again { "\n\nTu biblioteca, notas, marcadores y progreso se conservan." } else { "" }
        ),
        &[(100, if again { "Actualizar\nSustituye la versión instalada" } else { "Instalar\nMenú Inicio y «Abrir con Saga» para tus libros" })],
        Some(("Crear acceso directo en el escritorio", !again || p.desktop.join("Saga.lnk").exists())),
        false,
    );
    if b != 100 {
        return;
    }
    if let Err(e) = fs::create_dir_all(&p.dir) {
        tell("No se pudo instalar", &format!("No se pudo crear la carpeta:\n{}\n\n{e}", p.dir.display()), true);
        return;
    }
    if !write_app(&exe) {
        return;
    }
    // el propio instalador hace de desinstalador
    let un = p.dir.join("Desinstalar Saga.exe");
    if let Ok(me) = std::env::current_exe() {
        if me != un {
            let _ = fs::copy(&me, &un);
        }
    }

    // accesos
    let _ = fs::create_dir_all(&p.menu);
    let _ = shortcut(&p.menu.join("Saga.lnk"), &exe, &p.dir);
    let dl = p.desktop.join("Saga.lnk");
    if desk {
        let _ = fs::create_dir_all(&p.desktop);
        let _ = shortcut(&dl, &exe, &p.dir);
    } else {
        let _ = fs::remove_file(&dl);
    }

    // «Abrir con Saga», sin quitarle a nadie su programa predeterminado
    let exe_s = exe.display().to_string();
    let cls = format!("{}\\Classes", p.soft);
    reg_str(&format!("{cls}\\{PROGID}"), "", "Libro");
    reg_str(&format!("{cls}\\{PROGID}\\DefaultIcon"), "", &format!("\"{exe_s}\",0"));
    reg_str(&format!("{cls}\\{PROGID}\\shell\\open\\command"), "", &format!("\"{exe_s}\" \"%1\""));
    reg_str(&format!("{cls}\\Applications\\Saga.exe"), "FriendlyAppName", "Saga");
    reg_str(&format!("{cls}\\Applications\\Saga.exe\\shell\\open\\command"), "", &format!("\"{exe_s}\" \"%1\""));
    for e in EXTS {
        reg_str(&format!("{cls}\\.{e}\\OpenWithProgids"), PROGID, "");
        reg_str(&format!("{cls}\\Applications\\Saga.exe\\SupportedTypes"), &format!(".{e}"), "");
    }

    // Configuración > Aplicaciones
    let k = uninstall_key(&p);
    let un_s = un.display().to_string();
    reg_str(&k, "DisplayName", "Saga");
    reg_str(&k, "DisplayVersion", VERSION);
    reg_str(&k, "Publisher", "josecv222");
    reg_str(&k, "DisplayIcon", &format!("\"{exe_s}\",0"));
    reg_str(&k, "InstallLocation", &p.dir.display().to_string());
    reg_str(&k, "UninstallString", &format!("\"{un_s}\" --uninstall"));
    reg_str(&k, "QuietUninstallString", &format!("\"{un_s}\" --uninstall --quiet"));
    reg_str(&k, "URLInfoAbout", "https://github.com/josecv222/Saga");
    reg_dword(&k, "NoModify", 1);
    reg_dword(&k, "NoRepair", 1);
    reg_dword(&k, "EstimatedSize", (APP.len() as u32 * 2) / 1024);
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };

    let (b, _) = ask(
        "Saga está lista",
        "La encontrarás en el menú Inicio. Para añadir libros, arrástralos a la ventana o usa el botón «Añadir»; también puedes hacer clic derecho en un libro y elegir «Abrir con > Saga».",
        &[(101, "Abrir Saga")],
        None,
        false,
    );
    if b == 101 && !quiet() {
        let _ = std::process::Command::new(&exe).current_dir(&p.dir).spawn();
    }
}

// ------------------------------------------------------------ desinstalar

fn uninstall() {
    let p = places();
    let data = std::env::var("SAGA_SETUP_ROOT").map(|r| PathBuf::from(r).join("Datos")).unwrap_or_else(|_| known(&FOLDERID_LocalAppData).join("Saga"));
    let (b, wipe) = ask(
        "Desinstalar Saga",
        "Se quitarán la aplicación, sus accesos directos y «Abrir con Saga». Tus archivos de libros no se tocan nunca.\n\nLa biblioteca de Saga (portadas, notas, marcadores y progreso) se conserva, salvo que marques la casilla.",
        &[(100, "Desinstalar")],
        Some(("Borrar también la biblioteca", false)),
        true,
    );
    if b != 100 {
        return;
    }
    let exe = p.dir.join("Saga.exe");
    while exe.exists() && fs::remove_file(&exe).is_err() {
        let (b, _) = ask("Saga está abierta", "Ciérrala y pulsa Reintentar.", &[(100, "Reintentar")], None, true);
        if b != 100 {
            return;
        }
    }
    let _ = fs::remove_file(p.menu.join("Saga.lnk"));
    let _ = fs::remove_file(p.desktop.join("Saga.lnk"));
    let cls = format!("{}\\Classes", p.soft);
    reg_tree_del(&format!("{cls}\\{PROGID}"));
    reg_tree_del(&format!("{cls}\\Applications\\Saga.exe"));
    for e in EXTS {
        reg_value_del(&format!("{cls}\\.{e}\\OpenWithProgids"), PROGID);
    }
    reg_tree_del(&uninstall_key(&p));
    if std::env::var("SAGA_SETUP_ROOT").is_ok() {
        reg_tree_del(&p.soft);
    }
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
    if wipe {
        let _ = fs::remove_dir_all(&data);
    }
    // la carpeta (con este mismo .exe) se borra al salir
    let me = std::env::current_exe().ok();
    let inside = me.as_ref().is_some_and(|m| m.starts_with(&p.dir));
    for e in fs::read_dir(&p.dir).into_iter().flatten().flatten() {
        if Some(e.path()) != me {
            let _ = fs::remove_file(e.path());
        }
    }
    if inside {
        use std::os::windows::process::CommandExt;
        let d = p.dir.display().to_string();
        let _ = std::process::Command::new("cmd.exe")
            .raw_arg(format!("/c ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{d}\""))
            .creation_flags(0x0800_0000) // sin ventana
            .spawn();
    } else {
        let _ = fs::remove_dir(&p.dir);
    }
    tell("Saga se ha desinstalado", if wipe { "También se borró su biblioteca." } else { "Tu biblioteca se conserva por si vuelves a instalarla." }, false);
}

fn main() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let me = std::env::current_exe().ok();
    let named = me.as_ref().and_then(|m| m.file_name()).is_some_and(|n| n.to_string_lossy().to_lowercase().contains("desinstalar"));
    if std::env::args().any(|a| a == "--uninstall") || named {
        uninstall();
    } else {
        install();
    }
    let _ = PWSTR::null();
}

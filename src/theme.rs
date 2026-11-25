//! Ambientes de la aplicación y papeles del lector.
use crate::gfx::Color;

#[derive(Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
    pub dark: bool,
    pub bg: Color,
    pub panel: Color,
    pub raised: Color,
    pub line: Color,
    pub text: Color,
    pub text2: Color,
    pub text3: Color,
    pub accent: Color,
    pub on_accent: Color,
    /// Cuánto tiñe el fondo la portada enfocada.
    pub glow: f32,
}
#[allow(clippy::too_many_arguments)]
const fn mk(name: &'static str, dark: bool, bg: u32, panel: u32, raised: u32, text: u32, t2: u32, t3: u32, accent: u32, on: u32, glow: f32) -> Theme {
    let line = if dark { Color(1.0, 1.0, 1.0, 0.085) } else { Color(0.0, 0.0, 0.0, 0.10) };
    Theme { name, dark, bg: Color::hex(bg), panel: Color::hex(panel), raised: Color::hex(raised), line, text: Color::hex(text), text2: Color::hex(t2), text3: Color::hex(t3), accent: Color::hex(accent), on_accent: Color::hex(on), glow }
}
pub const THEMES: &[Theme] = &[
    mk("Penumbra", true, 0x0F1417, 0x171D20, 0x232C30, 0xECE7DC, 0xACB0AB, 0x7F8686, 0xE8C08A, 0x241A0C, 0.40),
    mk("Medianoche", true, 0x000000, 0x0C0D10, 0x191B21, 0xE4E4E8, 0x9FA0AB, 0x70727D, 0xA9B8E8, 0x0C1124, 0.26),
    mk("Bosque", true, 0x0E1511, 0x16201A, 0x223029, 0xE5EADC, 0xA7B4A6, 0x778578, 0xA5CC93, 0x10200E, 0.36),
    mk("Ciruela", true, 0x15111A, 0x1F1926, 0x2E2538, 0xEBE3EC, 0xB1A5B7, 0x83778B, 0xD9A8CF, 0x2A1226, 0.40),
    mk("Pergamino", false, 0xEADFC9, 0xF5ECDA, 0xFCF6E9, 0x35291C, 0x655441, 0x867459, 0x8F4F22, 0xFFF6E8, 0.34),
    mk("Alba", false, 0xE7EBEA, 0xF5F7F6, 0xFFFFFF, 0x1E2829, 0x4E5B5D, 0x74807F, 0x2E6468, 0xF2FBFA, 0.30),
];

#[derive(Clone, Copy)]
pub struct Paper {
    pub name: &'static str,
    pub bg: Color,
    pub ink: Color,
    pub dark: bool,
}
pub const PAPERS: &[Paper] = &[
    Paper { name: "Penumbra", bg: Color::hex(0x171C1E), ink: Color::hex(0xCDC7BA), dark: true },
    Paper { name: "Negro", bg: Color::hex(0x000000), ink: Color::hex(0xB2ACA0), dark: true },
    Paper { name: "Carbón", bg: Color::hex(0x2B2D2F), ink: Color::hex(0xD3CFC6), dark: true },
    Paper { name: "Bosque", bg: Color::hex(0x141C17), ink: Color::hex(0xC0CAB6), dark: true },
    Paper { name: "Ocaso", bg: Color::hex(0x1C1925), ink: Color::hex(0xCAC2D4), dark: true },
    Paper { name: "Sepia", bg: Color::hex(0xEEDFC4), ink: Color::hex(0x46351F), dark: false },
    Paper { name: "Papel", bg: Color::hex(0xF6F1E6), ink: Color::hex(0x2B2721), dark: false },
    Paper { name: "Blanco", bg: Color::hex(0xFFFFFF), ink: Color::hex(0x1B1B1B), dark: false },
];

pub const MARKS: [Color; 5] = [Color::hex(0xF2C94C), Color::hex(0x7CC88A), Color::hex(0x6FB3E8), Color::hex(0xE88BB0), Color::hex(0xB695E0)];
pub const POSTITS: [Color; 6] = [Color::hex(0xF6E27A), Color::hex(0xB9E29A), Color::hex(0x9DD4F0), Color::hex(0xF6B3C8), Color::hex(0xD3BDF2), Color::hex(0xF8C592)];

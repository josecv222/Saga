//! Iconos de línea sobre una rejilla de 24, dibujados por código.
macro_rules! icons {
    ($($n:ident $f:literal $p:literal;)*) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        #[repr(u16)]
        #[allow(dead_code)]
        pub enum Icon { $($n),* }
        impl Icon {
            /// Trazado y si va relleno.
            pub fn path(self) -> (&'static str, bool) { match self { $(Icon::$n => ($p, $f)),* } }
        }
    };
}
icons! {
    Search false "M4 11 A7 7 0 1 1 18 11 A7 7 0 1 1 4 11 Z M20.5 20.5 L16 16";
    Plus false "M12 5 V19 M5 12 H19";
    Close false "M6 6 L18 18 M18 6 L6 18";
    Min false "M5 12 H19";
    Max false "M6 6 H18 V18 H6 Z";
    Restore false "M9 9 V6 H18 V15 H15 M6 9 H15 V18 H6 Z";
    Gear false "M12 8.8 A3.2 3.2 0 1 1 12 15.2 A3.2 3.2 0 1 1 12 8.8 Z M12 2.8 V5.4 M12 18.6 V21.2 M2.8 12 H5.4 M18.6 12 H21.2 M5.5 5.5 L7.4 7.4 M16.6 16.6 L18.5 18.5 M5.5 18.5 L7.4 16.6 M16.6 7.4 L18.5 5.5 M12 5.4 A6.6 6.6 0 1 1 12 18.6 A6.6 6.6 0 1 1 12 5.4 Z";
    Filter false "M4 7 H20 M7 12 H17 M10 17 H14";
    Grid false "M4.5 4.5 H10 V10 H4.5 Z M14 4.5 H19.5 V10 H14 Z M4.5 14 H10 V19.5 H4.5 Z M14 14 H19.5 V19.5 H14 Z";
    Carousel false "M8.5 4.5 H15.5 V19.5 H8.5 Z M5 7 V17 M2 9 V15 M19 7 V17 M22 9 V15";
    Shelf false "M4 4.5 H7.5 V19 H4 Z M7.5 4.5 H11 V19 H7.5 Z M12.6 5.6 L16 4.7 L19.4 18.2 L16 19 Z M3 19.3 H21";
    Stats false "M5 20 V13 M10 20 V5 M15 20 V10 M20 20 V15";
    Book false "M12 6.5 C10 5 7 4.5 3.5 4.8 V18.3 C7 18 10 18.5 12 20 C14 18.5 17 18 20.5 18.3 V4.8 C17 4.5 14 5 12 6.5 Z M12 6.5 V20";
    Panel false "M4 5 H20 V19 H4 Z M10 5 V19";
    Heart false "M12 20 C5 15.5 3 11.8 3 8.8 A4.6 4.6 0 0 1 12 7 A4.6 4.6 0 0 1 21 8.8 C21 11.8 19 15.5 12 20 Z";
    HeartFill true "M12 20.5 C4.6 15.8 2.5 11.9 2.5 8.8 A5 5 0 0 1 12 6.6 A5 5 0 0 1 21.5 8.8 C21.5 11.9 19.4 15.8 12 20.5 Z";
    Star false "M12 3.5 L14.7 9 L20.7 9.9 L16.3 14.1 L17.4 20.1 L12 17.2 L6.6 20.1 L7.7 14.1 L3.3 9.9 L9.3 9 Z";
    StarFill true "M12 3 L14.9 8.7 L21.3 9.7 L16.7 14.2 L17.8 20.6 L12 17.6 L6.2 20.6 L7.3 14.2 L2.7 9.7 L9.1 8.7 Z";
    Pencil false "M4 20 L5 15.5 L16 4.5 L19.5 8 L8.5 19 Z M13.5 7 L17 10.5";
    Trash false "M4.5 7 H19.5 M9.5 7 V4.5 H14.5 V7 M6.5 7 L7.5 20 H16.5 L17.5 7 M10.2 10.8 V16.4 M13.8 10.8 V16.4";
    Left false "M15 5 L8 12 L15 19";
    Right false "M9 5 L16 12 L9 19";
    Up false "M5 15 L12 8 L19 15";
    Down false "M5 9 L12 16 L19 9";
    Check false "M4.5 12.5 L9.5 17.5 L19.5 6.5";
    Toc false "M9 6 H20 M9 12 H20 M9 18 H20 M4.5 6 H5 M4.5 12 H5 M4.5 18 H5";
    Type false "M3 18 L8 6 L13 18 M4.8 14 H11.2 M14.5 18 L17.8 10 L21 18 M15.8 15.2 H19.8";
    Sun false "M12 8 A4 4 0 1 1 12 16 A4 4 0 1 1 12 8 Z M12 2.5 V4.5 M12 19.5 V21.5 M2.5 12 H4.5 M19.5 12 H21.5 M5.3 5.3 L6.7 6.7 M17.3 17.3 L18.7 18.7 M5.3 18.7 L6.7 17.3 M17.3 6.7 L18.7 5.3";
    Moon false "M20 14.5 A8.5 8.5 0 1 1 9.5 4 A7 7 0 0 0 20 14.5 Z";
    Bookmark false "M6.5 4 H17.5 V20 L12 16 L6.5 20 Z";
    BookmarkFill true "M6 3.5 H18 V21 L12 16.8 L6 21 Z";
    Note false "M4.5 4.5 H19.5 V14.5 L14.5 19.5 H4.5 Z M19.5 14.5 H14.5 V19.5 M8 9 H16 M8 13 H12";
    Marker false "M4 20.5 H11 M7 16.5 L14.5 5 L19.5 8.3 L12 19.8 L7.5 19.6 Z M9 13.5 L14 16.8";
    Underline false "M7 4.5 V11 A5 5 0 0 0 17 11 V4.5 M5 20 H19";
    Copy false "M9 9 H19.5 V19.5 H9 Z M15 9 V4.5 H4.5 V15 H9";
    Speak false "M4 9.5 V14.5 H7.5 L12 18.5 V5.5 L7.5 9.5 Z M15.5 9 A4.2 4.2 0 0 1 15.5 15 M18 6.3 A8 8 0 0 1 18 17.7";
    Play true "M7 4.5 L19.5 12 L7 19.5 Z";
    Pause true "M6.5 5 H10.3 V19 H6.5 Z M13.7 5 H17.5 V19 H13.7 Z";
    Stop true "M6.5 6.5 H17.5 V17.5 H6.5 Z";
    Expand false "M4.5 9.5 V4.5 H9.5 M14.5 4.5 H19.5 V9.5 M19.5 14.5 V19.5 H14.5 M9.5 19.5 H4.5 V14.5";
    Eye false "M2.5 12 C5 7.5 8.3 5.5 12 5.5 C15.7 5.5 19 7.5 21.5 12 C19 16.5 15.7 18.5 12 18.5 C8.3 18.5 5 16.5 2.5 12 Z M12 9 A3 3 0 1 1 12 15 A3 3 0 1 1 12 9 Z";
    PageOne false "M6.5 4 H17.5 V20 H6.5 Z M9.5 8 H14.5 M9.5 12 H14.5 M9.5 16 H12.5";
    PageTwo false "M3 5 H12 V19 H3 Z M12 5 H21 V19 H12 Z M5.5 9 H9.5 M5.5 12.5 H9.5 M14.5 9 H18.5 M14.5 12.5 H18.5";
    ScrollV false "M7 3.5 H17 V20.5 H7 Z M12 8 V16 M9.5 13.5 L12 16 L14.5 13.5";
    ScrollH false "M3.5 6 H20.5 V18 H3.5 Z M8 12 H16 M13.5 9.5 L16 12 L13.5 14.5";
    File false "M6 3.5 H14 L18.5 8 V20.5 H6 Z M14 3.5 V8 H18.5";
    Folder false "M3.5 6 H9.5 L11.5 8.5 H20.5 V19 H3.5 Z";
    Image false "M4 5 H20 V19 H4 Z M4 16 L9 11 L13.5 15.5 L16 13 L20 17 M15.5 8 A1.2 1.2 0 1 1 15.5 10.4 A1.2 1.2 0 1 1 15.5 8 Z";
    Wand false "M4 20 L14.5 9.5 M17 3.5 V7.5 M15 5.5 H19 M20 12 V15 M18.5 13.5 H21.5 M9.5 4 V6.5 M8.3 5.2 H10.8";
    Tag false "M4 4.5 H11.5 L20 13 L13 20 L4.5 11.5 Z M8 8 H8.2";
    User false "M12 4.5 A3.8 3.8 0 1 1 12 12.1 A3.8 3.8 0 1 1 12 4.5 Z M4.5 20 C5 16.5 8 14.8 12 14.8 C16 14.8 19 16.5 19.5 20";
    Layers false "M12 4 L20.5 8.5 L12 13 L3.5 8.5 Z M3.5 12.5 L12 17 L20.5 12.5 M3.5 16.2 L12 20.7 L20.5 16.2";
    Calendar false "M4.5 6 H19.5 V20 H4.5 Z M4.5 10.5 H19.5 M8.5 3.5 V7.5 M15.5 3.5 V7.5";
    Clock false "M12 4 A8 8 0 1 1 12 20 A8 8 0 1 1 12 4 Z M12 8 V12.3 L15 14";
    More true "M5 10.4 A1.6 1.6 0 1 1 5 13.6 A1.6 1.6 0 1 1 5 10.4 Z M12 10.4 A1.6 1.6 0 1 1 12 13.6 A1.6 1.6 0 1 1 12 10.4 Z M19 10.4 A1.6 1.6 0 1 1 19 13.6 A1.6 1.6 0 1 1 19 10.4 Z";
    Sliders false "M4 7 H13 M17 7 H20 M15 5 V9 M4 17 H7 M11 17 H20 M9 15 V19";
    Globe false "M12 3.5 A8.5 8.5 0 1 1 12 20.5 A8.5 8.5 0 1 1 12 3.5 Z M3.5 12 H20.5 M12 3.5 C9 6.5 8.2 9.5 8.2 12 C8.2 14.5 9 17.5 12 20.5 C15 17.5 15.8 14.5 15.8 12 C15.8 9.5 15 6.5 12 3.5 Z";
    Sort false "M7 5 V19 M3.5 15.5 L7 19 L10.5 15.5 M13 7 H21 M13 12 H19 M13 17 H17";
    Zoom false "M4 11 A7 7 0 1 1 18 11 A7 7 0 1 1 4 11 Z M20.5 20.5 L16 16 M11 8 V14 M8 11 H14";
    ZoomOut false "M4 11 A7 7 0 1 1 18 11 A7 7 0 1 1 4 11 Z M20.5 20.5 L16 16 M8 11 H14";
    Export false "M12 4 V15 M8 8 L12 4 L16 8 M4.5 15 V20 H19.5 V15";
    Refresh false "M19.5 11 A7.6 7.6 0 1 0 18 16.5 M19.8 5.5 V11 H14.3";
    Info false "M12 3.8 A8.2 8.2 0 1 1 12 20.2 A8.2 8.2 0 1 1 12 3.8 Z M12 11 V16.5 M12 7.6 V7.8";
    Quote false "M10 7 C6.5 8 5 10.5 5 14 V17.5 H10 V12.5 H7.2 M19 7 C15.5 8 14 10.5 14 14 V17.5 H19 V12.5 H16.2";
}

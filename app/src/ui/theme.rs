use egui::Color32;
#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub nav_bg: Color32,
    pub nav_bg_hover: Color32,
    pub nav_active: Color32,
    pub nav_text: Color32,
    pub nav_text_muted: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_soft: Color32,
    pub button_bg: Color32,
    pub button_disabled: Color32,
    pub danger_button: Color32,
    pub danger_hover: Color32,
    pub page_bg: Color32,
    pub card_bg: Color32,
    pub card_border: Color32,
    pub row_hover: Color32,
    pub row_selected: Color32,
    pub header_bg: Color32,
    pub track_bg: Color32,
    pub segmented_bg: Color32,
    pub shadow: Color32,
    pub tooltip_bg: Color32,
    pub tooltip_text: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub text_faint: Color32,
    pub positive: Color32,
    pub negative: Color32,
    pub warning: Color32,
    pub pending_bg: Color32,
    pub pending_chip: Color32,
    pub success_bg: Color32,
    pub success_border: Color32,
    pub success_text: Color32,
}
pub fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}
impl Theme {
    pub fn new(dark: bool) -> Self {
        Self {
            dark,
            nav_bg: if dark { hex(0x101a2b) } else { hex(0x13233a) },
            nav_bg_hover: hex(0x1d3352),
            nav_active: hex(0x24508a),
            nav_text: hex(0xc9d6e8),
            nav_text_muted: if dark { hex(0x91a6c2) } else { hex(0x7f93ae) },
            accent: if dark { hex(0x8bbcff) } else { hex(0x1e6fd9) },
            accent_hover: if dark { hex(0x3479d7) } else { hex(0x185bb3) },
            accent_soft: if dark { hex(0x203959) } else { hex(0xe6f0fc) },
            button_bg: if dark { hex(0x286bc7) } else { hex(0x1e6fd9) },
            button_disabled: if dark { hex(0x3b4e68) } else { hex(0x9fb4cf) },
            danger_button: if dark { hex(0xb93342) } else { hex(0xc8323b) },
            danger_hover: hex(0xa8262e),
            page_bg: if dark { hex(0x121b29) } else { hex(0xf2f5f9) },
            card_bg: if dark { hex(0x1c293b) } else { hex(0xffffff) },
            card_border: if dark { hex(0x35465c) } else { hex(0xe2e8f0) },
            row_hover: if dark { hex(0x24354b) } else { hex(0xf6f9fd) },
            row_selected: if dark { hex(0x2a4364) } else { hex(0xe8f1fc) },
            header_bg: if dark { hex(0x202f43) } else { hex(0xf7f9fc) },
            track_bg: if dark { hex(0x304158) } else { hex(0xedf1f6) },
            segmented_bg: if dark { hex(0x111e30) } else { hex(0xe9eef5) },
            shadow: if dark {
                Color32::from_rgba_unmultiplied(0, 0, 0, 48)
            } else {
                Color32::from_rgba_unmultiplied(15, 30, 53, 16)
            },
            tooltip_bg: hex(0x13233a),
            tooltip_text: hex(0xffffff),
            text: if dark { hex(0xedf3fc) } else { hex(0x1b2635) },
            text_muted: if dark { hex(0xb0c0d6) } else { hex(0x66758a) },
            text_faint: if dark { hex(0x97abc6) } else { hex(0x97a3b4) },
            positive: if dark { hex(0x6cdaa3) } else { hex(0x13834a) },
            negative: if dark { hex(0xff8791) } else { hex(0xc8323b) },
            warning: if dark { hex(0xedc475) } else { hex(0xb7791f) },
            pending_bg: if dark { hex(0x322c21) } else { hex(0xfff7e6) },
            pending_chip: if dark { hex(0x514027) } else { hex(0xffe7b8) },
            success_bg: if dark { hex(0x203c32) } else { hex(0xeaf6ef) },
            success_border: if dark { hex(0x395f50) } else { hex(0xbfe3cc) },
            success_text: if dark { hex(0xa2e2be) } else { hex(0x1f5d3a) },
        }
    }
}

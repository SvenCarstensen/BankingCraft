use crate::syntax::{latin1_to_string, Segment};

/// One return code from HIRMG (global) or HIRMS (segment related).
#[derive(Debug, Clone)]
pub struct ReturnCode {
    pub code: String,
    pub reference_element: Option<String>,
    pub text: String,
    pub params: Vec<String>,
    /// Segment number this code refers to (HIRMS only).
    pub segment_ref: Option<u32>,
}

impl ReturnCode {
    pub fn is_error(&self) -> bool {
        self.code.starts_with('9')
    }
    pub fn is_warning(&self) -> bool {
        self.code.starts_with('3')
    }
}

#[derive(Debug, Clone, Default)]
pub struct Response {
    pub segments: Vec<Segment>,
    pub codes: Vec<ReturnCode>,
    pub dialog_id: Option<String>,
}

impl Response {
    pub fn new(segments: Vec<Segment>) -> Self {
        let mut codes = Vec::new();
        let mut dialog_id = None;
        for s in &segments {
            match s.code.as_str() {
                "HNHBK" => dialog_id = s.text(3, 0),
                "HIRMG" | "HIRMS" => {
                    for i in 1..=s.de_count() {
                        let f = s.de(i);
                        let get = |k: usize| f.get(k).and_then(|x| x.as_ref()).map(|b| latin1_to_string(b));
                        let Some(code) = get(0) else { continue };
                        codes.push(ReturnCode {
                            code,
                            reference_element: get(1).filter(|t| !t.is_empty()),
                            text: get(2).unwrap_or_default(),
                            params: (3..f.len()).filter_map(get).collect(),
                            segment_ref: if s.code == "HIRMS" { s.reference } else { None },
                        });
                    }
                }
                _ => {}
            }
        }
        Self { segments, codes, dialog_id }
    }

    pub fn find(&self, code: &str) -> impl Iterator<Item = &Segment> {
        let c = code.to_string();
        self.segments.iter().filter(move |s| s.code == c)
    }

    pub fn first(&self, code: &str) -> Option<&Segment> {
        self.segments.iter().find(|s| s.code == code)
    }

    /// Segments answering the request segment with number `seg_num`.
    pub fn answers_to(&self, seg_num: u32) -> impl Iterator<Item = &Segment> {
        self.segments.iter().filter(move |s| s.reference == Some(seg_num))
    }

    pub fn code(&self, code: &str) -> Option<&ReturnCode> {
        self.codes.iter().find(|c| c.code == code)
    }

    pub fn errors(&self) -> Vec<&ReturnCode> {
        self.codes.iter().filter(|c| c.is_error()).collect()
    }

    /// Touchdown point ("Aufsetzpunkt") for paginated answers to segment `seg_num`.
    pub fn touchdown(&self, seg_num: u32) -> Option<String> {
        self.codes
            .iter()
            .find(|c| c.code == "3040" && (c.segment_ref == Some(seg_num) || c.segment_ref.is_none()))
            .and_then(|c| c.params.first().cloned())
    }

    /// Security functions the user may use (code 3920).
    pub fn allowed_security_functions(&self) -> Vec<String> {
        self.codes
            .iter()
            .filter(|c| c.code == "3920")
            .flat_map(|c| c.params.clone())
            .collect()
    }

    pub fn summary(&self) -> String {
        self.codes
            .iter()
            .filter(|c| !c.code.starts_with('0') || c.code == "0030")
            .map(|c| format!("{} {}", c.code, c.text))
            .collect::<Vec<_>>()
            .join("\n")
    }
}


/// Validate a complete response before accepting bank data or successful order status.
pub fn validate_envelope(raw: &[u8], dialog_id: &str, message_number: u32) -> crate::Result<()> {
    use crate::error::Error;
    let segs=crate::syntax::parse_segments(raw)?;
    let invalid=||Error::Syntax(i18n::tr("Unvollständige oder widersprüchliche FinTS-Nachrichtenhülle").into());
    let head=segs.first().filter(|s|s.code=="HNHBK").ok_or_else(invalid)?;
    let tail=segs.last().filter(|s|s.code=="HNHBS").ok_or_else(invalid)?;
    if !raw.ends_with(b"'") || segs.iter().filter(|s|s.code=="HNHBK").count()!=1
        || segs.iter().filter(|s|s.code=="HNHBS").count()!=1
        || head.text_or_empty(1,0).parse::<usize>().ok()!=Some(raw.len())
        || head.text_or_empty(2,0)!="300"
        || head.text_or_empty(4,0).parse::<u32>().ok()!=Some(message_number)
        || tail.text_or_empty(1,0).parse::<u32>().ok()!=Some(message_number)
        || head.text(3,0).is_none()
        || (dialog_id!="0" && head.text_or_empty(3,0)!=dialog_id)
    {
        return Err(invalid());
    }
    if let Some(reference)=head.text(5,1) {
        if reference.parse::<u32>().ok()!=Some(message_number) { return Err(invalid()); }
    }
    Ok(())
}

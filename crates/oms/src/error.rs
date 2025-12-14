#![allow(dead_code)]

#[derive(Debug, Clone)]
pub struct OMSErr {
    module: &'static str,
    code: i32,
    msg: &'static str,
}

impl OMSErr {
    pub fn new(code: i32, msg: &'static str) -> Self {
        OMSErr {
            module: "OMS",
            code,
            msg,
        }
    }
}

impl std::fmt::Display for OMSErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OMSErr(code={})", self.code)
    }
}

impl std::error::Error for OMSErr {}

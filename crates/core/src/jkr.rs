//! `.jkr` container: raw DEFLATE (no zlib header) around `return {...}` Lua source.

use std::io::{Read, Write};
use std::path::Path;

use crate::lua::{self, Value};
use crate::Error;

pub fn decode(bytes: &[u8]) -> Result<String, Error> {
    let mut out = String::new();
    flate2::read::DeflateDecoder::new(bytes)
        .read_to_string(&mut out)
        .map_err(|e| Error::Decode(e.to_string()))?;
    Ok(out)
}

/// For building synthetic test fixtures. Real game files are never written.
pub fn encode(lua_src: &str) -> Vec<u8> {
    let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(lua_src.as_bytes()).expect("in-memory write");
    enc.finish().expect("in-memory write")
}

pub fn read(path: &Path) -> Result<Value, Error> {
    let bytes = std::fs::read(path).map_err(|e| Error::Io(path.to_path_buf(), e))?;
    let src = decode(&bytes)?;
    lua::parse(&src).map_err(Error::Lua)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let src = r#"return {["STATE"]=5,["GAME"]={["dollars"]=12,},}"#;
        let bytes = encode(src);
        assert_eq!(decode(&bytes).unwrap(), src);
        let v = lua::parse(&decode(&bytes).unwrap()).unwrap();
        assert_eq!(v.at("GAME.dollars").int(), Some(12));
    }
}

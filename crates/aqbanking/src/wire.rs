//! Bounded, secret-erasing frames shared with native/aqbanking/protocol.h.
use std::{collections::BTreeMap, io::{self, Read, Write}};
use zeroize::Zeroizing;

pub const LIMIT: usize = 32 * 1024 * 1024;
#[derive(Default)]
pub struct Frame(BTreeMap<String, Zeroizing<Vec<u8>>>);
fn invalid() -> io::Error { io::Error::new(io::ErrorKind::InvalidData,"Ungültige Antwort des Banking-Prozesses") }
impl Frame {
    pub fn entries(&self) -> impl Iterator<Item=(&str,&[u8])> { self.0.iter().map(|(k,v)|(k.as_str(),v.as_slice())) }
    pub fn size(&self) -> usize { self.0.iter().map(|(k,v)|6+k.len()+v.len()).sum() }
    pub fn new(op: &str) -> Self { let mut f=Self::default(); f.put("op",op); f }
    pub fn put(&mut self,key: &str,value: impl AsRef<[u8]>) { self.0.insert(key.into(),Zeroizing::new(value.as_ref().to_vec())); }
    pub fn bytes(&self,key: &str) -> &[u8] { self.0.get(key).map(|v|v.as_slice()).unwrap_or_default() }
    pub fn text(&self,key: &str) -> io::Result<&str> {
        let bytes=self.bytes(key);
        if bytes.contains(&0) { return Err(invalid()); }
        std::str::from_utf8(bytes).map_err(|_|invalid())
    }
    pub fn number(&self,key: &str) -> io::Result<i64> { self.text(key)?.parse().map_err(|_|invalid()) }
    pub fn write(&self,writer: &mut impl Write) -> io::Result<()> {
        let mut data=Zeroizing::new(Vec::new());
        if self.0.len()>128 { return Err(invalid()); }
        for (key,value) in &self.0 {
            if key.is_empty() || key.len()>255 || key.as_bytes().contains(&0) || value.len()>LIMIT { return Err(invalid()); }
            if data.len()+6+key.len()+value.len()>LIMIT { return Err(invalid()); }
            data.extend_from_slice(&(key.len() as u16).to_be_bytes()); data.extend_from_slice(key.as_bytes());
            data.extend_from_slice(&(value.len() as u32).to_be_bytes()); data.extend_from_slice(value);
        }
        writer.write_all(&(data.len() as u32).to_be_bytes())?; writer.write_all(&data)?; writer.flush()
    }
    pub fn read(reader: &mut impl Read) -> io::Result<Self> {
        let mut header=[0;4]; reader.read_exact(&mut header)?;
        let len=u32::from_be_bytes(header) as usize;
        if len>LIMIT { return Err(invalid()); }
        let mut data=Zeroizing::new(vec![0;len]); reader.read_exact(&mut data)?;
        Self::decode(&data)
    }
    pub fn decode(mut data: &[u8]) -> io::Result<Self> {
        let mut frame=Self::default();
        while !data.is_empty() {
            if data.len()<2 || frame.0.len()==128 { return Err(invalid()); }
            let n=u16::from_be_bytes([data[0],data[1]]) as usize; data=&data[2..];
            if n==0 || n>255 || data.len()<n+4 || data[..n].contains(&0) { return Err(invalid()); }
            let key=std::str::from_utf8(&data[..n]).map_err(|_|invalid())?.to_string(); data=&data[n..];
            let n=u32::from_be_bytes(data[..4].try_into().unwrap()) as usize; data=&data[4..];
            if n>LIMIT || data.len()<n || frame.0.contains_key(&key) { return Err(invalid()); }
            frame.put(&key,&data[..n]); data=&data[n..];
        }
        Ok(frame)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binary_roundtrip_and_hostile_frames() {
        let mut f=Frame::new("probe"); f.put("state",[0,255,1]); f.put("title","Überweisung");
        let mut bytes=Vec::new(); f.write(&mut bytes).unwrap();
        let restored=Frame::read(&mut bytes.as_slice()).unwrap(); assert_eq!(restored.bytes("state"),[0,255,1]);
        assert_eq!(restored.text("title").unwrap(),"Überweisung"); assert!(restored.text("state").is_err());
        for n in 0..bytes.len() { assert!(Frame::read(&mut &bytes[..n]).is_err()); }
        assert!(Frame::read(&mut &u32::MAX.to_be_bytes()[..]).is_err());
        let mut duplicate=bytes[4..].to_vec(); duplicate.extend_from_slice(&bytes[4..]); assert!(Frame::decode(&duplicate).is_err());
        for n in 0..4096 { let data=vec![(n%256) as u8;n]; let _=Frame::decode(&data); }
    }
}

use byteorder::{ByteOrder, LittleEndian};

#[derive(Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u64);

impl std::fmt::Debug for NodeId {
fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
write!(fmt, "NodeId({})", self.to_base32())
}
}

impl NodeId {
pub(crate) const ROOT: NodeId = NodeId(0);
pub fn is_root(&self) -> bool {
*self == NodeId::ROOT
}

pub fn to_base32(&self) -> String {
let mut b = [0; 8];
LittleEndian::write_u64(&mut b, self.0);
base32::encode(base32::Alphabet::Crockford, &b)
}
}

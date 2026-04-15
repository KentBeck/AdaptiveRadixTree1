pub(crate) const INLINE_PREFIX_CAP: usize = 22;

pub(crate) enum Prefix {
    Inline {
        len: u8,
        data: [u8; INLINE_PREFIX_CAP],
    },
    Heap(Box<[u8]>),
}

impl Prefix {
    pub(crate) fn empty() -> Self {
        Prefix::Inline {
            len: 0,
            data: [0; INLINE_PREFIX_CAP],
        }
    }

    pub(crate) fn from_slice(s: &[u8]) -> Self {
        if s.len() <= INLINE_PREFIX_CAP {
            let mut data = [0u8; INLINE_PREFIX_CAP];
            data[..s.len()].copy_from_slice(s);
            Prefix::Inline {
                len: s.len() as u8,
                data,
            }
        } else {
            Prefix::Heap(Box::from(s))
        }
    }

    pub(crate) fn as_slice(&self) -> &[u8] {
        match self {
            Prefix::Inline { len, data } => &data[..*len as usize],
            Prefix::Heap(bytes) => bytes,
        }
    }
}

impl Default for Prefix {
    fn default() -> Self {
        Prefix::empty()
    }
}

//! Firebird array identifiers are distinct from BLOB identifiers and payloads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ArrayId {
    pub high: i32,
    pub low: u32,
}
impl ArrayId {
    pub fn from_u64(value: u64) -> Self {
        Self {
            high: (value >> 32) as u32 as i32,
            low: value as u32,
        }
    }
    pub fn as_u64(self) -> u64 {
        ((self.high as u32 as u64) << 32) | self.low as u64
    }
}
/// Exact catalog identity reported by the prepared statement.
#[derive(Debug, PartialEq, Eq)]
pub struct ArrayOrigin {
    pub relation: String,
    pub field: String,
}

impl ArrayOrigin {
    pub fn from_names(relation: String, field: String) -> Option<std::sync::Arc<Self>> {
        if relation.is_empty() || field.is_empty() {
            return None;
        }
        Some(std::sync::Arc::new(Self { relation, field }))
    }
}

/// A bounded rectangular slice of a Firebird INTEGER array, in row-major order.
/// The SDL names are in the attachment charset and retain exact catalog spelling.
#[derive(Debug)]
pub struct IntegerArraySlice {
    sdl: Vec<u8>,
    count: usize,
}

impl IntegerArraySlice {
    pub const MAX_ELEMENTS: usize = 65_536;

    pub fn element_count(bounds: &[(i32, i32)]) -> Result<usize, crate::FbError> {
        if !(1..=16).contains(&bounds.len()) {
            return Err("An ARRAY slice needs 1 to 16 dimensions".into());
        }
        let mut count = 1usize;
        for &(lower, upper) in bounds {
            let width = usize::try_from(i64::from(upper) - i64::from(lower) + 1)
                .ok()
                .filter(|width| *width > 0)
                .ok_or("Invalid ARRAY slice bounds")?;
            count = count
                .checked_mul(width)
                .filter(|n| *n <= Self::MAX_ELEMENTS)
                .ok_or("ARRAY slice exceeds 65,536 elements; select a smaller range")?;
        }
        Ok(count)
    }

    pub fn new(
        relation: &[u8],
        field: &[u8],
        bounds: &[(i32, i32)],
    ) -> Result<Self, crate::FbError> {
        use crate::ibase::*;
        let count = Self::element_count(bounds)?;
        let mut sdl = vec![
            isc_sdl_version1 as u8,
            isc_sdl_struct as u8,
            1,
            8, /* BLR LONG */
            0,
        ];
        for (tag, name) in [(isc_sdl_relation, relation), (isc_sdl_field, field)] {
            let length = u8::try_from(name.len())
                .map_err(|_| "ARRAY identifier exceeds 255 encoded bytes")?;
            if name.is_empty() || name.contains(&0) {
                return Err("Invalid ARRAY identifier".into());
            }
            sdl.extend([tag as u8, length]);
            sdl.extend_from_slice(name);
        }
        for (dimension, &(lower, upper)) in bounds.iter().enumerate() {
            sdl.extend([isc_sdl_do2 as u8, dimension as u8]);
            for limit in [lower, upper] {
                sdl.push(isc_sdl_long_integer as u8);
                sdl.extend(limit.to_le_bytes());
            }
        }
        sdl.extend([
            isc_sdl_element as u8,
            1,
            isc_sdl_scalar as u8,
            0,
            bounds.len() as u8,
        ]);
        for dimension in 0..bounds.len() {
            sdl.extend([isc_sdl_variable as u8, dimension as u8]);
        }
        sdl.push(isc_sdl_eoc as u8);
        Ok(Self { sdl, count })
    }

    pub fn sdl(&self) -> &[u8] {
        &self.sdl
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn byte_len(&self) -> i32 {
        (self.count * size_of::<i32>()) as i32
    }
}

/// Explicit SDL element representation. Text descriptors include a charset;
/// integer descriptors retain their scale. No attachment-dependent coercion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArrayElement {
    pub blr: u8,
    pub scale: i8,
    pub length: u16,
    pub charset: u16,
}

impl ArrayElement {
    pub fn stride(self) -> Result<usize, crate::FbError> {
        Ok(match self.blr {
            7 => 2,
            8 | 10 | 12 | 13 => 4,
            16 | 24 | 27 | 35 | 28 => 8,
            25 | 26 => 16,
            29 => 12,
            23 => 1,
            15 | 41 if self.length > 0 => usize::from(self.length),
            _ => return Err("Unsupported ARRAY element descriptor".into()),
        })
    }

    fn descriptor(self) -> Result<Vec<u8>, crate::FbError> {
        self.stride()?;
        let mut bytes = vec![self.blr];
        match self.blr {
            7 | 8 | 16 | 26 => bytes.push(self.scale as u8),
            15 | 41 => {
                bytes.extend(self.charset.to_le_bytes());
                bytes.extend(self.length.to_le_bytes());
            }
            _ => {}
        }
        Ok(bytes)
    }
}

/// Bounded typed slice. Its buffer contains native, little-endian element slots.
/// Both supported desktop architectures use this representation. Wire clients
/// convert each slot to XDR; the native client passes the slots to isc_get_slice.
#[derive(Debug)]
pub struct ArraySlice {
    sdl: Vec<u8>,
    count: usize,
    element: ArrayElement,
    stride: usize,
}

impl ArraySlice {
    pub const MAX_BYTES: usize = 4 * 1024 * 1024;

    pub fn new(
        relation: &[u8],
        field: &[u8],
        bounds: &[(i32, i32)],
        element: ArrayElement,
    ) -> Result<Self, crate::FbError> {
        if cfg!(target_endian = "big") {
            return Err("Typed ARRAY buffers require a little-endian client".into());
        }
        let integer = IntegerArraySlice::new(relation, field, bounds)?;
        let stride = element.stride()?;
        integer
            .count
            .checked_mul(stride)
            .filter(|size| *size <= Self::MAX_BYTES)
            .ok_or("ARRAY slice exceeds 4 MiB; select a smaller range")?;
        let mut sdl = integer.sdl[..3].to_vec();
        sdl.extend(element.descriptor()?);
        sdl.extend_from_slice(&integer.sdl[5..]);
        Ok(Self {
            sdl,
            count: integer.count,
            element,
            stride,
        })
    }
    pub fn sdl(&self) -> &[u8] {
        &self.sdl
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn stride(&self) -> usize {
        self.stride
    }
    pub fn element(&self) -> ArrayElement {
        self.element
    }
    pub fn byte_len(&self) -> i32 {
        (self.count * self.stride) as i32
    }
    pub fn validate(&self, data: &[u8]) -> Result<(), crate::FbError> {
        if data.len() != self.byte_len() as usize {
            return Err("ARRAY payload size does not match its descriptor and bounds".into());
        }
        if self.element.blr == 41
            && data
                .chunks_exact(self.stride)
                .any(|slot| !slot.contains(&0))
        {
            return Err("An ARRAY CString element needs a terminating NUL".into());
        }
        if self.element.blr == 23 && data.iter().any(|byte| *byte > 1) {
            return Err("An ARRAY Boolean element must be zero or one".into());
        }
        Ok(())
    }
}

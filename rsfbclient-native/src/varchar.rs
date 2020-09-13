use std::{alloc, mem, ptr};

#[repr(C)]
/// Structure expected by the `fbclient`
pub struct InnerVarchar {
    len: u16,
    data: [u8; 0],
}

#[derive(Debug)]
/// Wrapper for a varchar buffer
pub struct Varchar {
    capacity: u16,
    ptr: ptr::NonNull<InnerVarchar>,
}

unsafe impl Send for Varchar {}

impl Varchar {
    /// Allocate a new varchar buffer
    pub fn new(capacity: u16) -> Self {
        let mut ptr = ptr::NonNull::new(unsafe {
            alloc::alloc(layout(capacity as usize)) as *mut InnerVarchar
        })
        .unwrap();

        unsafe { ptr.as_mut().len = 0 };

        Varchar { capacity, ptr }
    }

    /// Get the received bytes
    pub fn as_bytes(&self) -> &[u8] {
        let len = u16::min(self.capacity, unsafe { self.ptr.as_ref().len }) as usize;

        unsafe { self.ptr.as_ref().data.get_unchecked(..len) }
    }

    /// Get the pointer to the inner type
    pub fn as_ptr(&self) -> *mut InnerVarchar {
        self.ptr.as_ptr()
    }
}

impl Drop for Varchar {
    fn drop(&mut self) {
        unsafe {
            alloc::dealloc(self.ptr.as_ptr() as *mut u8, layout(self.capacity as usize));
        }
    }
}

fn layout(len: usize) -> alloc::Layout {
    alloc::Layout::from_size_align(
        mem::size_of::<InnerVarchar>() + len,
        mem::align_of::<InnerVarchar>(),
    )
    .unwrap()
}

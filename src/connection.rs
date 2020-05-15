///
/// Rust Firebird Client 
///
/// Connection functions 
///

use std::result::Result;
use std::os::raw::c_short;
use std::os::raw::c_char;
use std::ffi::CString;
use std::mem;

use super::ibase;
use super::error::FbError;

pub struct Connection {
    handle: *mut ibase::isc_db_handle
}

pub fn open(host: String, port: u32, db_name: String, user: String, pass: String) -> Result<Connection, FbError> {

    let handle: *mut u32 = &mut 0;

    unsafe {

        let mem_alloc = 1 + user.len() + 2 + pass.len() + 2;
        let mut dpb: *mut c_char = libc::malloc(mem_alloc) as *mut c_char;
        
        *dpb = ibase::isc_dpb_version1 as c_char;

        let c_user = match CString::new(user.clone()) {
            Ok(c) => c.as_ptr() as *const c_char,
            Err(e) => return Err(FbError { code: -1, msg: e.to_string() })
        };
        ibase::isc_modify_dpb(&mut dpb, &mut 0, ibase::isc_dpb_user_name as u16, c_user, user.len() as c_short);

        let c_pass = match CString::new(pass.clone()) {
            Ok(c) => c.as_ptr() as *const c_char,
            Err(e) => return Err(FbError { code: -1, msg: e.to_string() })
        };
        ibase::isc_modify_dpb(&mut dpb, &mut 0, ibase::isc_dpb_password as u16, c_pass, pass.len() as c_short);

        let host_db = format!("{}:{}", host, db_name);
        let c_host_db = match CString::new(host_db.clone()) {
            Ok(c) => c.as_ptr() as *const c_char,
            Err(e) => return Err(FbError { code: -1, msg: e.to_string() })
        };
        
        let status: *mut ibase::ISC_STATUS_ARRAY = libc::malloc(mem::size_of::<ibase::ISC_STATUS_ARRAY>()) as *mut ibase::ISC_STATUS_ARRAY;
        let host_db_len = host_db.len() as c_short;
        let dpb_len = 0 as c_short;

        if ibase::isc_attach_database(status, host_db_len, c_host_db, handle, dpb_len, dpb) != 0 {
            return Err(FbError::from_status(status)); 
        }
    }

    Ok(Connection {
        handle: handle
    })
}

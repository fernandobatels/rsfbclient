///
/// Rust Firebird Client 
///
/// Representation of a fetched row
///

use std::result::Result;
use std::ffi::CStr;

use super::ibase;
use super::error::FbError;
use super::statement::StatementFetch;

pub struct Row<'a> {
    pub stmt_ft: &'a StatementFetch
}

impl<'a> Row<'a> {

    /// Get the column value by the index
    pub fn get<T: ColumnAccess>(&self, idx: usize) -> Result<T, FbError> {

        unsafe {
            let xsqlda_ptr = *self.stmt_ft.xsqlda.as_ptr();        
            if idx as i16 >= (*xsqlda_ptr).sqld {
                return Err(FbError { code: -1, msg: "This index doesn't exists".to_string() });
            }
        }

        T::get(self, idx)
    }
}

/// Define the access to the row column
pub trait ColumnAccess where Self: Sized {
    
    /// Get the value of the row
    fn get(row: &Row, idx: usize) -> Result<Self, FbError>;
}

impl ColumnAccess for Option<i32> {

    fn get(row: &Row, idx: usize) -> Result<Option<i32>, FbError> {

        unsafe {
            let xsqlda_ptr = *row.stmt_ft.xsqlda.as_ptr();        
            let col = (*xsqlda_ptr).sqlvar[idx];

            if *col.sqlind < 0 {
                return Ok(None);
            }

            Ok(Some(*col.sqldata as i32))
        }
    }
}

impl ColumnAccess for i32 {

    fn get(row: &Row, idx: usize) -> Result<i32, FbError> {

        match ColumnAccess::get(row, idx) {
            Ok(val_op) => {
                match val_op {
                    Some(val) => Ok(val),
                    None => Err(FbError { code: -1, msg: "This is a null value. Use the Option<i32> to safe access this column and avoid errors".to_string() })
                }                
            },
            Err(e) => Err(e)
        }
    }
}

impl ColumnAccess for Option<f32> {

    fn get(row: &Row, idx: usize) -> Result<Option<f32>, FbError> {

        unsafe {
            let xsqlda_ptr = *row.stmt_ft.xsqlda.as_ptr();        
            let col = (*xsqlda_ptr).sqlvar[idx];

            if *col.sqlind < 0 {
                return Ok(None);
            }

            Ok(Some(*col.sqldata as f32))
        }
    }
}

impl ColumnAccess for f32 {

    fn get(row: &Row, idx: usize) -> Result<f32, FbError> {

        match ColumnAccess::get(row, idx) {
            Ok(val_op) => {
                match val_op {
                    Some(val) => Ok(val),
                    None => Err(FbError { code: -1, msg: "This is a null value. Use the Option<f32> to safe access this column and avoid errors".to_string() })
                }                
            },
            Err(e) => Err(e)
        }
    }
}

impl ColumnAccess for Option<String> {

    fn get(row: &Row, idx: usize) -> Result<Option<String>, FbError> {

        unsafe {
            let xsqlda_ptr = *row.stmt_ft.xsqlda.as_ptr();        
            let col = (*xsqlda_ptr).sqlvar[idx];

            if *col.sqlind < 0 {
                return Ok(None);
            }

            let vary = &*(col.sqldata as *const ibase::PARAMVARY); 
            if vary.vary_length == 0 {
                return Ok(Some("".to_string()));
            }

            // TODO: change the vary_string to a *mut c_char!
            let str_bytes = &vary.vary_string[0..vary.vary_length as usize];
            let c_str = CStr::from_bytes_with_nul_unchecked(str_bytes);

            match c_str.to_str() {
                Ok(st) => Ok(Some(st.to_string())),
                Err(e) => Err(FbError { code: -1, msg: format!("{}", e) })
            }
        }
    }
}

impl ColumnAccess for String {

    fn get(row: &Row, idx: usize) -> Result<String, FbError> {

        match ColumnAccess::get(row, idx) {
            Ok(val_op) => {
                match val_op {
                    Some(val) => Ok(val),
                    None => Err(FbError { code: -1, msg: "This is a null value. Use the Option<String> to safe access this column and avoid errors".to_string() })
                }                
            },
            Err(e) => Err(e)
        }
    }
}

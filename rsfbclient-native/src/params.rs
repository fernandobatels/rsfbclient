use std::{mem, ptr};

use crate::{
    ibase::{self, IBase},
    status::Status,
    xsqlda::XSqlDa,
};
use rsfbclient_core::{Charset, FbError, SqlType, MAX_TEXT_LENGTH};

use ParamBufferData::*;

/// Stores the data needed to send the parameters
pub struct Params {
    /// Input xsqlda
    pub(crate) xsqlda: Option<XSqlDa>,

    /// Data used by the xsqlda above
    _buffers: Vec<ParamBuffer>,
}

impl Params {
    /// Validate and set the parameters of a statement
    pub(crate) fn new<T: IBase>(
        db_handle: &mut ibase::isc_db_handle,
        tr_handle: &mut ibase::isc_tr_handle,
        ibase: &T,
        status: &mut Status,
        stmt_handle: &mut ibase::isc_stmt_handle,
        infos: Vec<SqlType>,
        charset: &Charset,
    ) -> Result<Self, FbError> {
        let params = if !infos.is_empty() {
            let mut xsqlda = XSqlDa::new(infos.len() as i16);

            let ok = unsafe {
                ibase.isc_dsql_describe_bind()(&mut status[0], stmt_handle, 1, &mut *xsqlda)
            };
            if ok != 0 {
                return Err(status.as_error(ibase));
            }

            if xsqlda.sqld != xsqlda.sqln {
                return Err(format!(
                    "Wrong parameter count, you passed {}, but the sql contains needs {} params",
                    xsqlda.sqln, xsqlda.sqld
                )
                .into());
            }

            let mut buffers = vec![];

            for (col, info) in infos.into_iter().enumerate() {
                buffers.push(ParamBuffer::from_parameter(
                    info,
                    xsqlda
                        .get_xsqlvar_mut(col)
                        .ok_or_else(|| FbError::from("Error getting the xsqlvar"))?,
                    db_handle,
                    tr_handle,
                    ibase,
                    &charset,
                )?);
            }

            Self {
                _buffers: buffers,
                xsqlda: Some(xsqlda),
            }
        } else {
            Self {
                _buffers: vec![],
                xsqlda: None,
            }
        };

        Ok(params)
    }

    // /// For use when there is no statement, cant verify the number of parameters ahead of time
    // pub fn new_immediate(infos: Vec<Param>) -> Self {
    //     if !infos.is_empty() {
    //         let mut xsqlda = XSqlDa::new(infos.len() as i16);

    //         xsqlda.sqld = xsqlda.sqln;

    //         let buffers = infos
    //             .into_iter()
    //             .enumerate()
    //             .map(|(col, info)| {
    //                 ParamBuffer::from_parameter(info, xsqlda.get_xsqlvar_mut(col).unwrap())
    //             })
    //             .collect();

    //         Self {
    //             _buffers: buffers,
    //             xsqlda: Some(xsqlda),
    //         }
    //     } else {
    //         Self {
    //             _buffers: vec![],
    //             xsqlda: None,
    //         }
    //     }
    // }
}

/// Data for the input XSQLVAR
pub struct ParamBuffer {
    /// Buffer for the parameter data
    _buffer: ParamBufferData,

    /// Null indicator
    _nullind: Box<i16>,
}

/// Data for the input XSQLVAR.
/// Holds the data in the format expected by the `fbclient`
enum ParamBufferData {
    Text(Box<[u8]>),

    Integer(Box<i64>),

    Floating(Box<f64>),

    Timestamp(Box<ibase::ISC_TIMESTAMP>),

    Null,

    Binary(Box<[u8]>),

    /// Only works in fb >= 3.0
    Boolean(Box<i8>),
}

impl ParamBufferData {
    /// Get a pointer to the underlying data
    fn as_mut_ptr(&mut self) -> *mut ibase::ISC_SCHAR {
        match self {
            Text(s) => s.as_ptr() as _,
            Integer(i) => &**i as *const _ as _,
            Floating(f) => &**f as *const _ as _,
            Timestamp(ts) => &**ts as *const _ as _,
            Null => ptr::null_mut(),
            Binary(b) => b.as_ptr() as _,
            Boolean(b) => &**b as *const _ as _,
        }
    }
}

impl ParamBuffer {
    /// Allocate a buffer from a value to use in an input (parameter) XSQLVAR
    pub fn from_parameter<T: IBase>(
        info: SqlType,
        var: &mut ibase::XSQLVAR,
        db: &mut ibase::isc_db_handle,
        tr: &mut ibase::isc_tr_handle,
        ibase: &T,
        charset: &Charset,
    ) -> Result<Self, FbError> {
        let mut null = 0;

        let (sqltype, sqlsubtype) = info.sql_type_and_subtype();
        var.sqltype = sqltype as i16;
        var.sqlsubtype = sqlsubtype as i16;
        var.sqlscale = 0;

        let (size, mut buffer) = match info {
            SqlType::Text(s) => {
                let bytes = charset.encode(s)?;

                let bytes = if bytes.len() > MAX_TEXT_LENGTH {
                    binary_to_blob(&bytes, db, tr, ibase)?
                } else {
                    bytes.into_owned()
                };

                (bytes.len(), Text(bytes.into_boxed_slice()))
            }

            SqlType::Integer(i) => (mem::size_of_val(&i), Integer(Box::new(i))),

            SqlType::Floating(f) => (mem::size_of_val(&f), Floating(Box::new(f))),

            #[cfg(feature = "date_time")]
            SqlType::Timestamp(dt) => {
                let ts = rsfbclient_core::date_time::encode_timestamp(dt);

                (mem::size_of_val(&ts), Timestamp(Box::new(ts)))
            }

            SqlType::Null => {
                null = -1;
                (0, Null)
            }

            SqlType::Binary(bin) => {
                let bytes = binary_to_blob(&bin, db, tr, ibase)?;
                (bytes.len(), Binary(bytes.into_boxed_slice()))
            }

            SqlType::Boolean(bo) => (mem::size_of::<i8>(), Boolean(Box::new(bo as i8))),
        };

        let mut nullind = Box::new(null);
        var.sqlind = &mut *nullind;

        var.sqldata = buffer.as_mut_ptr();
        var.sqllen = size as i16;

        Ok(ParamBuffer {
            _buffer: buffer,
            _nullind: nullind,
        })
    }
}

// Convert the binary vec to a blob
fn binary_to_blob<T: IBase>(
    bytes: &[u8],
    db_handle: &mut ibase::isc_db_handle,
    tr_handle: &mut ibase::isc_tr_handle,
    ibase: &T,
) -> Result<Vec<u8>, FbError> {
    let mut status = Status::default();
    let mut handle = 0;

    let mut blob_id = ibase::GDS_QUAD_t {
        gds_quad_high: 0,
        gds_quad_low: 0,
    };

    unsafe {
        if ibase.isc_create_blob()(
            &mut status[0],
            db_handle,
            tr_handle,
            &mut handle,
            &mut blob_id,
        ) != 0
        {
            return Err(status.as_error(ibase));
        }
    }

    // Assert that the handle is valid
    debug_assert_ne!(handle, 0);

    unsafe {
        if ibase.isc_put_segment()(
            &mut status[0],
            &mut handle,
            bytes.len() as u16,
            bytes.as_ptr() as *mut std::os::raw::c_char,
        ) != 0
        {
            return Err(status.as_error(ibase));
        }
    }

    unsafe {
        if ibase.isc_close_blob()(&mut status[0], &mut handle) != 0 {
            return Err(status.as_error(ibase));
        }
    }

    Ok([
        blob_id.gds_quad_high.to_ne_bytes(),
        blob_id.gds_quad_low.to_ne_bytes(),
    ]
    .concat())
}

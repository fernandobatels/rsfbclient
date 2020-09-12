use crate::{ibase, ibase::IBase, status::Status, xsqlda::XSqlDa};
use rsfbclient_core::{Charset, FbError, Param, MAX_TEXT_LENGTH};

/// Stores the data needed to send the parameters
pub struct Params {
    /// Input xsqlda
    pub(crate) xsqlda: Option<XSqlDa>,

    /// Data used by the xsqlda above
    _buffers: Vec<ParamBuffer>,
}

impl Params {
    /// Validate and set the parameters of a statement
    pub(crate) fn new(
        db_handle: &mut ibase::isc_db_handle,
        tr_handle: &mut ibase::isc_tr_handle,
        ibase: &ibase::IBase,
        status: &mut Status,
        stmt_handle: &mut ibase::isc_stmt_handle,
        infos: Vec<Param>,
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
                    xsqlda.get_xsqlvar_mut(col).unwrap(),
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
    _buffer: Box<[u8]>,

    /// Null indicator
    _nullind: Box<i16>,
}

impl ParamBuffer {
    /// Allocate a buffer from a value to use in an input (parameter) XSQLVAR
    pub fn from_parameter(
        info: Param,
        var: &mut ibase::XSQLVAR,
        db: &mut ibase::isc_db_handle,
        tr: &mut ibase::isc_tr_handle,
        ibase: &IBase,
        charset: &Charset,
    ) -> Result<Self, FbError> {
        let mut null = 0;

        let (sqltype, sqlsubtype) = info.sql_type_and_subtype();
        var.sqltype = sqltype as i16;
        var.sqlsubtype = sqlsubtype as i16;
        var.sqlscale = 0;

        let mut buffer = match info {
            Param::Text(s) => {
                let bytes = charset.encode(s)?;

                if bytes.len() > MAX_TEXT_LENGTH {
                    binary_to_blob(&bytes, db, tr, ibase)?
                } else {
                    bytes.into_owned()
                }
            }
            Param::Integer(i) => i.to_ne_bytes().to_vec(),
            Param::Floating(f) => f.to_ne_bytes().to_vec(),
            Param::Timestamp(ts) => [
                ts.timestamp_date.to_ne_bytes(),
                ts.timestamp_time.to_ne_bytes(),
            ]
            .concat(),
            Param::Null => {
                null = -1;
                vec![]
            }
            Param::Binary(bin) => binary_to_blob(&bin, db, tr, ibase)?,
            Param::Boolean(bo) => (bo as i8).to_ne_bytes().to_vec(),
        }
        .into_boxed_slice();

        let mut nullind = Box::new(null);
        var.sqlind = &mut *nullind;

        var.sqldata = buffer.as_mut_ptr() as *mut _;
        var.sqllen = buffer.len() as i16;

        Ok(ParamBuffer {
            _buffer: buffer,
            _nullind: nullind,
        })
    }
}

// Convert the binary vec to a blob
fn binary_to_blob(
    bytes: &[u8],
    db_handle: &mut ibase::isc_db_handle,
    tr_handle: &mut ibase::isc_tr_handle,
    ibase: &IBase,
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
            return Err(status.as_error(&ibase));
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
            return Err(status.as_error(&ibase));
        }
    }

    unsafe {
        if ibase.isc_close_blob()(&mut status[0], &mut handle) != 0 {
            return Err(status.as_error(&ibase));
        }
    }

    Ok([
        blob_id.gds_quad_high.to_ne_bytes(),
        blob_id.gds_quad_low.to_ne_bytes(),
    ]
    .concat())
}

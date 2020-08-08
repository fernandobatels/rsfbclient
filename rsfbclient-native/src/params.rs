use crate::{ibase, status::Status, xsqlda::XSqlDa};
use rsfbclient_core::{FbError, Param};

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
        ibase: &ibase::IBase,
        status: &mut Status,
        stmt_handle: &mut ibase::isc_stmt_handle,
        infos: Vec<Param>,
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
                return Err(FbError {
                    code: -1,
                    msg: format!("Wrong parameter count, you passed {}, but the sql contains needs {} params", xsqlda.sqln, xsqlda.sqld)
                });
            }

            let buffers = infos
                .into_iter()
                .enumerate()
                .map(|(col, info)| {
                    ParamBuffer::from_parameter(info, xsqlda.get_xsqlvar_mut(col).unwrap())
                })
                .collect();

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
    pub fn from_parameter(info: Param, var: &mut ibase::XSQLVAR) -> Self {
        let mut null = 0;

        var.sqltype = info.sql_type() as i16;
        var.sqlscale = 0;

        let mut buffer = match info {
            Param::Text(s) => s.into_bytes(),
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
        };

        let mut nullind = Box::new(null);
        var.sqlind = &mut *nullind;

        var.sqldata = buffer.as_mut_ptr() as *mut _;
        var.sqllen = buffer.len() as i16;

        ParamBuffer {
            _buffer: buffer.into_boxed_slice(),
            _nullind: nullind,
        }
    }
}

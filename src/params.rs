use crate::{ibase, statement::Statement, xsqlda::XSqlDa, FbError};

/// Stores the data needed to send the parameters
pub struct Params {
    /// Input xsqlda
    pub(crate) xsqlda: XSqlDa,

    /// Data used by the xsqlda above
    _buffers: Vec<ParamBuffer>,
}

impl Params {
    /// Validate and set the parameters of a statement
    pub(crate) fn new(stmt: &mut Statement, infos: Vec<ParamInfo>) -> Result<Self, FbError> {
        let status = &stmt.tr.conn.status;
        let mut xsqlda = XSqlDa::new(infos.len() as i16);

        let buffers = if !infos.is_empty() {
            let ok = unsafe {
                ibase::isc_dsql_describe_bind(
                    status.borrow_mut().as_mut_ptr(),
                    &mut stmt.handle,
                    1,
                    &mut *xsqlda,
                )
            };
            if ok != 0 {
                return Err(status.borrow().as_error());
            }

            if xsqlda.sqld != xsqlda.sqln {
                return Err(FbError {
                    code: -1,
                    msg: format!("Wrong parameter count, you passed {}, but the sql contains needs {} params", xsqlda.sqln, xsqlda.sqld)
                });
            }

            infos
                .into_iter()
                .enumerate()
                .map(|(col, info)| {
                    ParamBuffer::from_parameter(info, xsqlda.get_xsqlvar_mut(col).unwrap())
                })
                .collect()
        } else {
            vec![]
        };

        Ok(Self {
            _buffers: buffers,
            xsqlda,
        })
    }

    /// For use when there is no statement, cant verify the number of parameters ahead of time
    pub fn new_immediate(infos: Vec<ParamInfo>) -> Self {
        let mut xsqlda = XSqlDa::new(infos.len() as i16);

        xsqlda.sqld = xsqlda.sqln;

        let buffers = infos
            .into_iter()
            .enumerate()
            .map(|(col, info)| {
                ParamBuffer::from_parameter(info, xsqlda.get_xsqlvar_mut(col).unwrap())
            })
            .collect();

        Self {
            _buffers: buffers,
            xsqlda,
        }
    }
}

/// Data for the input XSQLVAR
pub struct ParamBuffer {
    /// Buffer for the parameter data
    _buffer: Vec<u8>,

    /// Null indicator
    _nullind: Box<i16>,
}

impl ParamBuffer {
    /// Allocate a buffer from a value to use in an input (parameter) XSQLVAR
    pub fn from_parameter(mut info: ParamInfo, var: &mut ibase::XSQLVAR) -> Self {
        let null = if info.null { -1 } else { 0 };

        let mut nullind = Box::new(null);
        var.sqlind = &mut *nullind;

        var.sqltype = info.sqltype;
        var.sqlscale = 0;

        var.sqldata = info.buffer.as_mut_ptr() as *mut i8;
        var.sqllen = info.buffer.len() as i16;

        ParamBuffer {
            _buffer: info.buffer,
            _nullind: nullind,
        }
    }
}

/// Data used to build the input XSQLVAR
pub struct ParamInfo {
    pub(crate) sqltype: i16,
    pub(crate) buffer: Vec<u8>,
    pub(crate) null: bool,
}

/// Implemented for types that can be sent as parameters
pub trait ToParam {
    fn to_info(self) -> ParamInfo;
}

impl ToParam for String {
    fn to_info(self) -> ParamInfo {
        let buffer = Vec::from(self);

        ParamInfo {
            sqltype: ibase::SQL_TEXT as i16 + 1,
            buffer,
            null: false,
        }
    }
}

impl ToParam for i64 {
    fn to_info(self) -> ParamInfo {
        let buffer = self.to_le_bytes().to_vec();

        ParamInfo {
            sqltype: ibase::SQL_INT64 as i16 + 1,
            buffer,
            null: false,
        }
    }
}

/// Implements AsParam for integers
macro_rules! impl_param_int {
    ( $( $t: ident ),+ ) => {
        $(
            impl ToParam for $t {
                fn to_info(self) -> ParamInfo {
                    (self as i64).to_info()
                }
            }
        )+
    };
}

impl_param_int!(i32, u32, i16, u16, i8, u8);

impl ToParam for f64 {
    fn to_info(self) -> ParamInfo {
        let buffer = self.to_le_bytes().to_vec();

        ParamInfo {
            sqltype: ibase::SQL_DOUBLE as i16 + 1,
            buffer,
            null: false,
        }
    }
}

impl ToParam for f32 {
    fn to_info(self) -> ParamInfo {
        (self as f64).to_info()
    }
}

/// Implements for all nullable variants
impl<T> ToParam for Option<T>
where
    T: ToParam,
{
    fn to_info(self) -> ParamInfo {
        if let Some(v) = self {
            v.to_info()
        } else {
            ParamInfo {
                sqltype: ibase::SQL_NULL as i16 + 1,
                buffer: vec![],
                null: true,
            }
        }
    }
}

/// Implements for all borrowed variants (&str, Cow and etc)
impl<T, B> ToParam for &B
where
    B: ToOwned<Owned = T> + ?Sized,
    T: core::borrow::Borrow<B> + ToParam,
{
    fn to_info(self) -> ParamInfo {
        self.to_owned().to_info()
    }
}

/// Implemented for types that represents a list of parameters
pub trait IntoParams {
    fn to_params(self) -> Vec<ParamInfo>;
}

/// Represents no parameters
impl IntoParams for () {
    fn to_params(self) -> Vec<ParamInfo> {
        vec![]
    }
}

/// Generates IntoParams implementations for a tuple
macro_rules! impl_into_params {
    ($([$t: ident, $v: ident]),+) => {
        impl<$($t),+> IntoParams for ($($t,)+)
        where
            $( $t: ToParam, )+
        {
            fn to_params(self) -> Vec<ParamInfo> {
                let ( $($v,)+ ) = self;

                vec![ $(
                    $v.to_info(),
                )+ ]
            }
        }
    };
}

/// Generates FromRow implementations for various tuples
macro_rules! impls_into_params {
    ([$t: ident, $v: ident]) => {
        impl_into_params!([$t, $v]);
    };

    ([$t: ident, $v: ident], $([$ts: ident, $vs: ident]),+ ) => {
        impls_into_params!($([$ts, $vs]),+);

        impl_into_params!([$t, $v], $([$ts, $vs]),+);
    };
}

impls_into_params!(
    [A, a],
    [B, b],
    [C, c],
    [D, d],
    [E, e],
    [F, f],
    [G, g],
    [H, h],
    [I, i],
    [J, j],
    [K, k],
    [L, l],
    [M, m],
    [N, n],
    [O, o]
);

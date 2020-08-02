use crate::{ibase, statement::StatementData, xsqlda::XSqlDa, Connection, FbError};

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
        conn: &Connection,
        stmt: &mut StatementData,
        infos: Vec<ParamInfo>,
    ) -> Result<Self, FbError> {
        let ibase = &conn.ibase;
        let status = &conn.status;

        let params = if !infos.is_empty() {
            let mut xsqlda = XSqlDa::new(infos.len() as i16);

            let ok = unsafe {
                ibase.isc_dsql_describe_bind()(
                    status.borrow_mut().as_mut_ptr(),
                    &mut stmt.handle,
                    1,
                    &mut *xsqlda,
                )
            };
            if ok != 0 {
                return Err(status.borrow().as_error(ibase));
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

    /// For use when there is no statement, cant verify the number of parameters ahead of time
    pub fn new_immediate(infos: Vec<ParamInfo>) -> Self {
        if !infos.is_empty() {
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
                xsqlda: Some(xsqlda),
            }
        } else {
            Self {
                _buffers: vec![],
                xsqlda: None,
            }
        }
    }
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
    pub fn from_parameter(mut info: ParamInfo, var: &mut ibase::XSQLVAR) -> Self {
        let null = if info.null { -1 } else { 0 };

        let mut nullind = Box::new(null);
        var.sqlind = &mut *nullind;

        var.sqltype = info.sqltype;
        var.sqlscale = 0;

        var.sqldata = info.buffer.as_mut_ptr() as *mut _;
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
    pub(crate) buffer: Box<[u8]>,
    pub(crate) null: bool,
}

/// Implemented for types that can be sent as parameters
pub trait ToParam {
    fn to_info(self) -> ParamInfo;
}

impl ToParam for String {
    fn to_info(self) -> ParamInfo {
        let buffer = Vec::from(self).into_boxed_slice();

        ParamInfo {
            sqltype: ibase::SQL_TEXT as i16 + 1,
            buffer,
            null: false,
        }
    }
}

impl ToParam for i64 {
    fn to_info(self) -> ParamInfo {
        let buffer = self.to_le_bytes().to_vec().into_boxed_slice();

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
        let buffer = self.to_le_bytes().to_vec().into_boxed_slice();

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
                buffer: Box::new([]),
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

#[cfg(test)]
mod test {
    use crate::{prelude::*, Connection, FbError};
    use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

    #[test]
    fn dates() -> Result<(), FbError> {
        let mut conn = conn();

        conn.execute("DROP TABLE PDATES", ()).ok();
        conn.execute(
            "CREATE TABLE PDATES (ref char(1), a date, b timestamp, c time)",
            (),
        )?;

        conn.execute(
            "insert into pdates (ref, a) values ('a', ?)",
            (NaiveDate::from_ymd(2009, 08, 07),),
        )?;
        let val_exists: Option<(i16,)> = conn.query_first(
            "select 1 from pdates where ref = 'a' and a = '2009-08-07'",
            (),
        )?;
        assert!(val_exists.is_some());

        conn.execute(
            "insert into pdates (ref, b) values ('b', ?)",
            (NaiveDate::from_ymd(2009, 08, 07).and_hms(11, 32, 25),),
        )?;
        let val_exists: Option<(i16,)> = conn.query_first(
            "select 1 from pdates where ref = 'b' and b = '2009-08-07 11:32:25'",
            (),
        )?;
        assert!(val_exists.is_some());

        conn.execute(
            "insert into pdates (ref, c) values ('c', ?)",
            (NaiveTime::from_hms(11, 22, 33),),
        )?;
        let val_exists: Option<(i16,)> = conn.query_first(
            "select 1 from pdates where ref = 'c' and c = '11:22:33'",
            (),
        )?;
        assert!(val_exists.is_some());

        Ok(())
    }

    #[test]
    fn strings() -> Result<(), FbError> {
        let mut conn = conn();

        conn.execute("DROP TABLE PSTRINGS", ()).ok();
        conn.execute(
            "CREATE TABLE PSTRINGS (ref char(1), a varchar(10), b varchar(10))",
            (),
        )?;

        conn.execute(
            "insert into pstrings (ref, a) values ('a', ?)",
            ("firebird",),
        )?;
        let val_exists: Option<(i16,)> = conn.query_first(
            "select 1 from pstrings where ref = 'a' and a = 'firebird'",
            (),
        )?;
        assert!(val_exists.is_some());

        conn.execute(
            "insert into pstrings (ref, b) values ('b', ?)",
            ("firebird",),
        )?;
        let val_exists: Option<(i16,)> = conn.query_first(
            "select 1 from pstrings where ref = 'b' and b = 'firebird  '",
            (),
        )?;
        assert!(val_exists.is_some());

        Ok(())
    }

    #[test]
    fn fixed_points() -> Result<(), FbError> {
        let mut conn = conn();

        conn.execute("DROP TABLE PFIXEDS", ()).ok();
        conn.execute(
            "CREATE TABLE PFIXEDS (ref char(1), a numeric(2, 2), b decimal(2, 2))",
            (),
        )?;

        conn.execute("insert into pfixeds (ref, a) values ('a', ?)", (22.33,))?;
        let val_exists: Option<(i16,)> =
            conn.query_first("select 1 from pfixeds where ref = 'a' and a = 22.33", ())?;
        assert!(val_exists.is_some());

        conn.execute("insert into pfixeds (ref, b) values ('b', ?)", (22.33,))?;
        let val_exists: Option<(i16,)> =
            conn.query_first("select 1 from pfixeds where ref = 'b' and b = 22.33", ())?;
        assert!(val_exists.is_some());

        Ok(())
    }

    #[test]
    fn float_points() -> Result<(), FbError> {
        let mut conn = conn();

        conn.execute("DROP TABLE PFLOATS", ()).ok();
        conn.execute(
            "CREATE TABLE PFLOATS (ref char(1), a float, b double precision)",
            (),
        )?;

        conn.execute("insert into pfloats (ref, a) values ('a', ?)", (3.402E38,))?;
        let val_exists: Option<(i16,)> = conn.query_first(
            "select 1 from pfloats where ref = 'a' and a = cast(3.402E38 as float)",
            (),
        )?;
        assert!(val_exists.is_some());

        conn.execute(
            "insert into pfloats (ref, b) values ('b', ?)",
            (2.225e-300,),
        )?;
        let val_exists: Option<(i16,)> = conn.query_first(
            "select 1 from pfloats where ref = 'b' and b = 2.225E-300",
            (),
        )?;
        assert!(val_exists.is_some());

        Ok(())
    }

    #[test]
    fn ints() -> Result<(), FbError> {
        let mut conn = conn();

        conn.execute("DROP TABLE PINTEGERS", ()).ok();
        conn.execute(
            "CREATE TABLE PINTEGERS (ref char(1), a smallint, b int, c bigint)",
            (),
        )?;

        conn.execute(
            "insert into pintegers (ref, a) values ('a', ?)",
            (i16::MIN,),
        )?;
        let val_exists: Option<(i16,)> =
            conn.query_first("select 1 from pintegers where ref = 'a' and a = -32768", ())?;
        assert!(val_exists.is_some());

        conn.execute(
            "insert into pintegers (ref, b) values ('b', ?)",
            (i32::MIN,),
        )?;
        let val_exists: Option<(i16,)> = conn.query_first(
            "select 1 from pintegers where ref = 'b' and b = -2147483648",
            (),
        )?;
        assert!(val_exists.is_some());

        conn.execute(
            "insert into pintegers (ref, c) values ('c', ?)",
            (i64::MIN,),
        )?;
        let val_exists: Option<(i16,)> = conn.query_first(
            "select 1 from pintegers where ref = 'c' and c = -9223372036854775808",
            (),
        )?;
        assert!(val_exists.is_some());

        Ok(())
    }

    fn conn() -> Connection {
        #[cfg(not(feature = "dynamic_loading"))]
        let conn = crate::ConnectionBuilder::default()
            .connect()
            .expect("Error on connect the test database");

        #[cfg(feature = "dynamic_loading")]
        let conn = crate::ConnectionBuilder::with_client("./fbclient.lib")
            .expect("Error finding fbclient lib")
            .connect()
            .expect("Error on connect the test database");

        conn
    }
}

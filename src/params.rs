use crate::{ibase, statement::StatementData, Connection, FbError};

use bytes::{BufMut, Bytes, BytesMut};
use ParamType::*;

/// Maximum parameter data length
const MAX_DATA_LENGTH: u16 = 32767;

/// Data for the parameters to send in the wire
pub struct Params {
    /// Definitions of the data types
    pub(crate) blr: Bytes,
    /// Actual values of the data
    pub(crate) values: Bytes,
}

impl Params {
    /// Validate and set the parameters of a statement
    pub(crate) fn new(
        conn: &Connection,
        stmt: &mut StatementData,
        infos: Vec<ParamInfo>,
    ) -> Result<Self, FbError> {
        // TODO: Verify if parameter length match the sql
        params_to_blr(&infos)
    }

    /// For use when there is no statement, cant verify the number of parameters ahead of time
    pub fn new_immediate(infos: Vec<ParamInfo>) -> Result<Self, FbError> {
        params_to_blr(&infos)
    }
}

/// Convert the parameters to a blr (binary representation)
pub fn params_to_blr(params: &[ParamInfo]) -> Result<Params, FbError> {
    let mut blr = BytesMut::with_capacity(256);
    let mut values = BytesMut::with_capacity(256);

    blr.put_slice(&[
        ibase::blr::VERSION5,
        ibase::blr::BEGIN,
        ibase::blr::MESSAGE,
        0, // Message index
    ]);
    // Message length, * 2 as there is 1 msg for the param type and another for the nullind
    blr.put_u16_le(params.len() as u16 * 2);

    for p in params {
        let len = p.buffer.len() as u16;
        if len > MAX_DATA_LENGTH {
            return Err("Parameter too big! Not supported yet".into());
        }

        values.put_slice(&p.buffer);
        if len % 4 != 0 {
            values.put_slice(&[0; 4][..4 - (len as usize % 4)])
        }
        values.put_i32_le(if p.null { -1 } else { 0 });

        match p.sqltype {
            ParamType::Text => {
                blr.put_u8(ibase::blr::TEXT);
                blr.put_u16_le(len);
            }

            ParamType::Integer => blr.put_slice(&[
                ibase::blr::INT64,
                0, // Scale
            ]),

            ParamType::Floating => blr.put_u8(ibase::blr::DOUBLE),

            ParamType::Timestamp => blr.put_u8(ibase::blr::TIMESTAMP),

            ParamType::Null => {
                // Represent as empty text
                blr.put_u8(ibase::blr::TEXT);
                blr.put_u16_le(0);
            }
        }
        // Nullind
        blr.put_slice(&[ibase::blr::SHORT, 0]);
    }

    blr.put_slice(&[ibase::blr::END, ibase::blr::EOC]);

    Ok(Params {
        blr: blr.freeze(),
        values: values.freeze(),
    })
}

/// Data used to build the input XSQLVAR
pub struct ParamInfo {
    pub(crate) sqltype: ParamType,
    pub(crate) buffer: Box<[u8]>,
    pub(crate) null: bool,
}

pub enum ParamType {
    /// Send as text
    Text,

    /// Send as bigint
    Integer,

    /// Send as double
    Floating,

    // Send as timestamp
    Timestamp,

    // Send as null
    Null,
}

/// Implemented for types that can be sent as parameters
pub trait ToParam {
    fn to_info(self) -> ParamInfo;
}

impl ToParam for String {
    fn to_info(self) -> ParamInfo {
        let buffer = Vec::from(self).into_boxed_slice();

        ParamInfo {
            sqltype: Text,
            buffer,
            null: false,
        }
    }
}

impl ToParam for i64 {
    fn to_info(self) -> ParamInfo {
        let buffer = self.to_be_bytes().to_vec().into_boxed_slice();

        ParamInfo {
            sqltype: Integer,
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
        let buffer = self.to_be_bytes().to_vec().into_boxed_slice();

        ParamInfo {
            sqltype: Floating,
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
                sqltype: Null,
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
    use chrono::{NaiveDate, NaiveTime};

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
            (NaiveDate::from_ymd(2009, 8, 7),),
        )?;
        let val_exists: Option<(i16,)> = conn.query_first(
            "select 1 from pdates where ref = 'a' and a = '2009-08-07'",
            (),
        )?;
        assert!(val_exists.is_some());

        conn.execute(
            "insert into pdates (ref, b) values ('b', ?)",
            (NaiveDate::from_ymd(2009, 8, 7).and_hms(11, 32, 25),),
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

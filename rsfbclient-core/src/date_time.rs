use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike};

use crate::{
    error::{err_column_null, err_type_conv},
    ibase, Column, ColumnToVal, ColumnType, FbError, IntoParam, Param,
};

const FRACTION_TO_NANOS: u32 = 1e9 as u32 / ibase::ISC_TIME_SECONDS_PRECISION;

/// Convert a numeric day to [day, month, year]. (Ported from the firebird source)
///
/// Calenders are divided into 4 year cycles: 3 non-leap years, and 1 leap year.
/// Each cycle takes 365*4 + 1 == 1461 days.
/// There is a further cycle of 100 4 year cycles.
/// Every 100 years, the normally expected leap year is not present. Every 400 years it is.
/// This cycle takes 100 * 1461 - 3 == 146097 days.
/// The origin of the constant 2400001 is unknown.
/// The origin of the constant 1721119 is unknown.
/// The difference between 2400001 and 1721119 is the
/// number of days from 0/0/0000 to our base date of 11/xx/1858 (678882)
/// The origin of the constant 153 is unknown.
///
/// This whole routine has problems with ndates less than -678882 (Approx 2/1/0000).
pub fn decode_date(date: ibase::ISC_DATE) -> NaiveDate {
    let mut nday = date;

    nday += 2400001 - 1721119;

    let century = (4 * nday - 1) / 146097;
    nday = 4 * nday - 1 - 146097 * century;

    let mut day = nday / 4;
    nday = (4 * day + 3) / 1461;
    day = 4 * day + 3 - 1461 * nday;
    day = (day + 4) / 4;

    let mut month = (5 * day - 3) / 153;
    day = 5 * day - 3 - 153 * month;
    day = (day + 5) / 5;

    let mut year = 100 * century + nday;

    if month < 10 {
        month += 3;
    } else {
        month -= 9;
        year += 1;
    };

    chrono::NaiveDate::from_ymd(year, month as u32, day as u32)
}

/// Convert a [day, month, year] to numeric day (Ported from the firebird source)
pub fn encode_date(date: NaiveDate) -> ibase::ISC_DATE {
    let day = date.day() as i64;
    let mut month = date.month() as i64;
    let mut year = date.year() as i64;

    if month > 2 {
        month -= 3;
    } else {
        month += 9;
        year -= 1;
    }

    let c = year / 100;
    let ya = year - 100 * c;

    ((146097 * c) as i64 / 4 + (1461 * ya) / 4 + (153 * month + 2) / 5 + day + 1721119 - 2400001)
        as ibase::ISC_DATE
}

/// Convert a numeric time to [hours, minutes, seconds] (Ported from the firebird source)
pub fn decode_time(time: ibase::ISC_TIME) -> NaiveTime {
    let mut ntime = time;

    let hours = ntime / (3600 * ibase::ISC_TIME_SECONDS_PRECISION);
    ntime %= 3600 * ibase::ISC_TIME_SECONDS_PRECISION;

    let minutes = ntime / (60 * ibase::ISC_TIME_SECONDS_PRECISION);
    ntime %= 60 * ibase::ISC_TIME_SECONDS_PRECISION;

    let seconds = ntime / ibase::ISC_TIME_SECONDS_PRECISION;

    let fraction = ntime % ibase::ISC_TIME_SECONDS_PRECISION;

    chrono::NaiveTime::from_hms_nano(hours, minutes, seconds, fraction * FRACTION_TO_NANOS)
}

/// Convert a [hours, minutes, seconds] to a numeric time (Ported from the firebird source)
pub fn encode_time(time: chrono::NaiveTime) -> ibase::ISC_TIME {
    let hours = time.hour();
    let minutes = time.minute();
    let seconds = time.second();
    let fraction = time.nanosecond() / FRACTION_TO_NANOS;

    ((hours * 60 + minutes) * 60 + seconds) * ibase::ISC_TIME_SECONDS_PRECISION + fraction
}

/// Convert a numeric timestamp to a DateTime
pub fn decode_timestamp(ts: ibase::ISC_TIMESTAMP) -> NaiveDateTime {
    decode_date(ts.timestamp_date).and_time(decode_time(ts.timestamp_time))
}

/// Convert a DateTime to a numeric timestamp
pub fn encode_timestamp(dt: NaiveDateTime) -> ibase::ISC_TIMESTAMP {
    ibase::ISC_TIMESTAMP {
        timestamp_date: encode_date(dt.date()),
        timestamp_time: encode_time(dt.time()),
    }
}

impl IntoParam for NaiveDateTime {
    fn into_param(self) -> Param {
        Param::Timestamp(encode_timestamp(self))
    }
}

impl IntoParam for NaiveDate {
    fn into_param(self) -> Param {
        // Mimics firebird conversion
        self.and_time(NaiveTime::from_hms(0, 0, 0)).into_param()
    }
}

impl IntoParam for NaiveTime {
    fn into_param(self) -> Param {
        // Mimics firebird conversion
        chrono::Utc::today().naive_utc().and_time(self).into_param()
    }
}

impl ColumnToVal<chrono::NaiveDate> for Column {
    fn to_val(self) -> Result<chrono::NaiveDate, FbError> {
        let col = self.value.ok_or_else(|| err_column_null("NaiveDate"))?;

        match col {
            ColumnType::Timestamp(ts) => Ok(crate::date_time::decode_timestamp(ts).date()),

            _ => err_type_conv(col, "NaiveDate"),
        }
    }
}

impl ColumnToVal<chrono::NaiveTime> for Column {
    fn to_val(self) -> Result<chrono::NaiveTime, FbError> {
        let col = self.value.ok_or_else(|| err_column_null("NaiveTime"))?;

        match col {
            ColumnType::Timestamp(ts) => Ok(crate::date_time::decode_timestamp(ts).time()),

            _ => err_type_conv(col, "NaiveTime"),
        }
    }
}

impl ColumnToVal<chrono::NaiveDateTime> for Column {
    fn to_val(self) -> Result<chrono::NaiveDateTime, FbError> {
        let col = self.value.ok_or_else(|| err_column_null("NaiveDateTime"))?;

        match col {
            ColumnType::Timestamp(ts) => Ok(crate::date_time::decode_timestamp(ts)),

            _ => err_type_conv(col, "NaiveDateTime"),
        }
    }
}

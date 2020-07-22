use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use std::mem;

use crate::{
    ibase,
    params::{ParamInfo, ToParam},
    status::err_buffer_len,
    FbError,
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

impl ToParam for NaiveDateTime {
    fn to_info(self) -> ParamInfo {
        let mut buffer = Vec::with_capacity(mem::size_of::<ibase::ISC_TIMESTAMP>());
        let timestamp = encode_timestamp(self);

        buffer.extend(&timestamp.timestamp_date.to_le_bytes());
        buffer.extend(&timestamp.timestamp_time.to_le_bytes());

        ParamInfo {
            sqltype: ibase::SQL_TIMESTAMP as i16 + 1,
            buffer,
            null: false,
        }
    }
}

/// Interprets a timestamp value from a buffer
pub fn timestamp_from_buffer(buffer: &[u8]) -> Result<chrono::NaiveDateTime, FbError> {
    let len = mem::size_of::<ibase::ISC_TIMESTAMP>();
    if buffer.len() < len {
        return err_buffer_len(len, buffer.len(), "NaiveDateTime");
    }

    let date = ibase::ISC_TIMESTAMP {
        timestamp_date: ibase::ISC_DATE::from_le_bytes([
            buffer[0], buffer[1], buffer[2], buffer[3],
        ]),
        timestamp_time: ibase::ISC_TIME::from_le_bytes([
            buffer[4], buffer[5], buffer[6], buffer[7],
        ]),
    };

    Ok(decode_timestamp(date))
}

impl ToParam for NaiveDate {
    fn to_info(self) -> ParamInfo {
        // Mimics firebird conversion
        self.and_time(NaiveTime::from_hms(0, 0, 0)).to_info()
    }
}

impl ToParam for NaiveTime {
    fn to_info(self) -> ParamInfo {
        // Mimics firebird conversion
        chrono::Utc::today().naive_utc().and_time(self).to_info()
    }
}

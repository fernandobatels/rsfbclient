//!
//! Rust Firebird Client
//!
//! fbclient functions
//!

#![allow(non_upper_case_globals, dead_code, non_camel_case_types)]

use super::common::*;

#[cfg(feature = "dynamic_loading")]
lazy_static::lazy_static! {
    /// Loads the fbclient library dynamically
    static ref LIB: libloading::Library = libloading::Library::new("./fbclient.lib").unwrap();
}

#[cfg(feature = "dynamic_loading")]
/// Registers the fbclient functions loaded from the library
macro_rules! parse_functions {
    ( $(
        extern "C" {
            pub fn $name:ident($($params:tt)*) $( -> $ret:ty )*;
        }
    )* ) => {
        $(
            lazy_static::lazy_static! {
                pub static ref $name: libloading::Symbol<'static, unsafe extern "C" fn($($params)*) $( -> $ret )*> =
                    unsafe { LIB.get(stringify!($name).as_bytes()).unwrap() };
            }
        )*
    };
}

#[cfg(not(feature = "dynamic_loading"))]
/// Just passes the functions as they are
macro_rules! parse_functions {
    ( $( $tokens:tt )* ) => {
        $( $tokens )*
    }
}

parse_functions! {
    extern "C" {
        pub fn isc_attach_database(
            arg1: *mut ISC_STATUS,
            arg2: ::std::os::raw::c_short,
            arg3: *const ISC_SCHAR,
            arg4: *mut isc_db_handle,
            arg5: ::std::os::raw::c_short,
            arg6: *const ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_array_gen_sdl(
            arg1: *mut ISC_STATUS,
            arg2: *const ISC_ARRAY_DESC,
            arg3: *mut ISC_SHORT,
            arg4: *mut ISC_UCHAR,
            arg5: *mut ISC_SHORT,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_array_get_slice(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: *mut ISC_QUAD,
            arg5: *const ISC_ARRAY_DESC,
            arg6: *mut ::std::os::raw::c_void,
            arg7: *mut ISC_LONG,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_array_lookup_bounds(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: *const ISC_SCHAR,
            arg5: *const ISC_SCHAR,
            arg6: *mut ISC_ARRAY_DESC,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_array_lookup_desc(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: *const ISC_SCHAR,
            arg5: *const ISC_SCHAR,
            arg6: *mut ISC_ARRAY_DESC,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_array_set_desc(
            arg1: *mut ISC_STATUS,
            arg2: *const ISC_SCHAR,
            arg3: *const ISC_SCHAR,
            arg4: *const ::std::os::raw::c_short,
            arg5: *const ::std::os::raw::c_short,
            arg6: *const ::std::os::raw::c_short,
            arg7: *mut ISC_ARRAY_DESC,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_array_put_slice(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: *mut ISC_QUAD,
            arg5: *const ISC_ARRAY_DESC,
            arg6: *mut ::std::os::raw::c_void,
            arg7: *mut ISC_LONG,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_blob_default_desc(
            arg1: *mut ISC_BLOB_DESC,
            arg2: *const ISC_UCHAR,
            arg3: *const ISC_UCHAR,
        );
    }
    extern "C" {
        pub fn isc_blob_gen_bpb(
            arg1: *mut ISC_STATUS,
            arg2: *const ISC_BLOB_DESC,
            arg3: *const ISC_BLOB_DESC,
            arg4: ::std::os::raw::c_ushort,
            arg5: *mut ISC_UCHAR,
            arg6: *mut ::std::os::raw::c_ushort,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_blob_info(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_blob_handle,
            arg3: ::std::os::raw::c_short,
            arg4: *const ISC_SCHAR,
            arg5: ::std::os::raw::c_short,
            arg6: *mut ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_blob_lookup_desc(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: *const ISC_UCHAR,
            arg5: *const ISC_UCHAR,
            arg6: *mut ISC_BLOB_DESC,
            arg7: *mut ISC_UCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_blob_set_desc(
            arg1: *mut ISC_STATUS,
            arg2: *const ISC_UCHAR,
            arg3: *const ISC_UCHAR,
            arg4: ::std::os::raw::c_short,
            arg5: ::std::os::raw::c_short,
            arg6: ::std::os::raw::c_short,
            arg7: *mut ISC_BLOB_DESC,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_cancel_blob(arg1: *mut ISC_STATUS, arg2: *mut isc_blob_handle) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_cancel_events(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut ISC_LONG,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_close_blob(arg1: *mut ISC_STATUS, arg2: *mut isc_blob_handle) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_commit_retaining(arg1: *mut ISC_STATUS, arg2: *mut isc_tr_handle) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_commit_transaction(arg1: *mut ISC_STATUS, arg2: *mut isc_tr_handle) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_create_blob(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: *mut isc_blob_handle,
            arg5: *mut ISC_QUAD,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_create_blob2(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: *mut isc_blob_handle,
            arg5: *mut ISC_QUAD,
            arg6: ::std::os::raw::c_short,
            arg7: *const ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_create_database(
            arg1: *mut ISC_STATUS,
            arg2: ::std::os::raw::c_short,
            arg3: *const ISC_SCHAR,
            arg4: *mut isc_db_handle,
            arg5: ::std::os::raw::c_short,
            arg6: *const ISC_SCHAR,
            arg7: ::std::os::raw::c_short,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_database_info(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: ::std::os::raw::c_short,
            arg4: *const ISC_SCHAR,
            arg5: ::std::os::raw::c_short,
            arg6: *mut ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_decode_date(arg1: *const ISC_QUAD, arg2: *mut ::std::os::raw::c_void);
    }
    extern "C" {
        pub fn isc_decode_sql_date(arg1: *const ISC_DATE, arg2: *mut ::std::os::raw::c_void);
    }
    extern "C" {
        pub fn isc_decode_sql_time(arg1: *const ISC_TIME, arg2: *mut ::std::os::raw::c_void);
    }
    extern "C" {
        pub fn isc_decode_timestamp(arg1: *const ISC_TIMESTAMP, arg2: *mut ::std::os::raw::c_void);
    }
    extern "C" {
        pub fn isc_detach_database(arg1: *mut ISC_STATUS, arg2: *mut isc_db_handle) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_drop_database(arg1: *mut ISC_STATUS, arg2: *mut isc_db_handle) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_allocate_statement(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_stmt_handle,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_alloc_statement2(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_stmt_handle,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_describe(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_stmt_handle,
            arg3: ::std::os::raw::c_ushort,
            arg4: *mut XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_describe_bind(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_stmt_handle,
            arg3: ::std::os::raw::c_ushort,
            arg4: *mut XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_exec_immed2(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: ::std::os::raw::c_ushort,
            arg5: *const ISC_SCHAR,
            arg6: ::std::os::raw::c_ushort,
            arg7: *const XSQLDA,
            arg8: *const XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_execute(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_tr_handle,
            arg3: *mut isc_stmt_handle,
            arg4: ::std::os::raw::c_ushort,
            arg5: *const XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_execute2(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_tr_handle,
            arg3: *mut isc_stmt_handle,
            arg4: ::std::os::raw::c_ushort,
            arg5: *const XSQLDA,
            arg6: *const XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_execute_immediate(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: ::std::os::raw::c_ushort,
            arg5: *const ISC_SCHAR,
            arg6: ::std::os::raw::c_ushort,
            arg7: *const XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_fetch(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_stmt_handle,
            arg3: ::std::os::raw::c_ushort,
            arg4: *const XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_finish(arg1: *mut isc_db_handle) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_free_statement(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_stmt_handle,
            arg3: ::std::os::raw::c_ushort,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_insert(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_stmt_handle,
            arg3: ::std::os::raw::c_ushort,
            arg4: *mut XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_prepare(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_tr_handle,
            arg3: *mut isc_stmt_handle,
            arg4: ::std::os::raw::c_ushort,
            arg5: *const ISC_SCHAR,
            arg6: ::std::os::raw::c_ushort,
            arg7: *mut XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_set_cursor_name(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_stmt_handle,
            arg3: *const ISC_SCHAR,
            arg4: ::std::os::raw::c_ushort,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_sql_info(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_stmt_handle,
            arg3: ::std::os::raw::c_short,
            arg4: *const ISC_SCHAR,
            arg5: ::std::os::raw::c_short,
            arg6: *mut ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_encode_date(arg1: *const ::std::os::raw::c_void, arg2: *mut ISC_QUAD);
    }
    extern "C" {
        pub fn isc_encode_sql_date(arg1: *const ::std::os::raw::c_void, arg2: *mut ISC_DATE);
    }
    extern "C" {
        pub fn isc_encode_sql_time(arg1: *const ::std::os::raw::c_void, arg2: *mut ISC_TIME);
    }
    extern "C" {
        pub fn isc_encode_timestamp(arg1: *const ::std::os::raw::c_void, arg2: *mut ISC_TIMESTAMP);
    }
    extern "C" {
        pub fn isc_event_block(
            arg1: *mut *mut ISC_UCHAR,
            arg2: *mut *mut ISC_UCHAR,
            arg3: ISC_USHORT,
            ...
        ) -> ISC_LONG;
    }
    extern "C" {
        pub fn isc_event_block_a(
            arg1: *mut *mut ISC_SCHAR,
            arg2: *mut *mut ISC_SCHAR,
            arg3: ISC_USHORT,
            arg4: *mut *mut ISC_SCHAR,
        ) -> ISC_USHORT;
    }
    extern "C" {
        pub fn isc_event_block_s(
            arg1: *mut *mut ISC_SCHAR,
            arg2: *mut *mut ISC_SCHAR,
            arg3: ISC_USHORT,
            arg4: *mut *mut ISC_SCHAR,
            arg5: *mut ISC_USHORT,
        );
    }
    extern "C" {
        pub fn isc_event_counts(
            arg1: *mut ISC_ULONG,
            arg2: ::std::os::raw::c_short,
            arg3: *mut ISC_UCHAR,
            arg4: *const ISC_UCHAR,
        );
    }
    extern "C" {
        pub fn isc_expand_dpb(arg1: *mut *mut ISC_SCHAR, arg2: *mut ::std::os::raw::c_short, ...);
    }
    extern "C" {
        pub fn isc_modify_dpb(
            arg1: *mut *mut ISC_SCHAR,
            arg2: *mut ::std::os::raw::c_short,
            arg3: ::std::os::raw::c_ushort,
            arg4: *const ISC_SCHAR,
            arg5: ::std::os::raw::c_short,
        ) -> ::std::os::raw::c_int;
    }
    extern "C" {
        pub fn isc_free(arg1: *mut ISC_SCHAR) -> ISC_LONG;
    }
    extern "C" {
        pub fn isc_get_segment(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_blob_handle,
            arg3: *mut ::std::os::raw::c_ushort,
            arg4: ::std::os::raw::c_ushort,
            arg5: *mut ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_get_slice(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: *mut ISC_QUAD,
            arg5: ::std::os::raw::c_short,
            arg6: *const ISC_SCHAR,
            arg7: ::std::os::raw::c_short,
            arg8: *const ISC_LONG,
            arg9: ISC_LONG,
            arg10: *mut ::std::os::raw::c_void,
            arg11: *mut ISC_LONG,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_interprete(arg1: *mut ISC_SCHAR, arg2: *mut *mut ISC_STATUS) -> ISC_LONG;
    }
    extern "C" {
        pub fn fb_interpret(
            arg1: *mut ISC_SCHAR,
            arg2: ::std::os::raw::c_uint,
            arg3: *mut *const ISC_STATUS,
        ) -> ISC_LONG;
    }
    extern "C" {
        pub fn isc_open_blob(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: *mut isc_blob_handle,
            arg5: *mut ISC_QUAD,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_open_blob2(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: *mut isc_blob_handle,
            arg5: *mut ISC_QUAD,
            arg6: ISC_USHORT,
            arg7: *const ISC_UCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_prepare_transaction2(
            arg1: *mut ISC_STATUS_ARRAY,
            arg2: *mut isc_tr_handle,
            arg3: ISC_USHORT,
            arg4: *const ISC_UCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_print_sqlerror(arg1: ISC_SHORT, arg2: *const ISC_STATUS);
    }
    extern "C" {
        pub fn isc_print_status(arg1: *const ISC_STATUS_ARRAY) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_put_segment(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_blob_handle,
            arg3: ::std::os::raw::c_ushort,
            arg4: *const ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_put_slice(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: *mut ISC_QUAD,
            arg5: ::std::os::raw::c_short,
            arg6: *const ISC_SCHAR,
            arg7: ::std::os::raw::c_short,
            arg8: *const ISC_LONG,
            arg9: ISC_LONG,
            arg10: *mut ::std::os::raw::c_void,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_que_events(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut ISC_LONG,
            arg4: ::std::os::raw::c_short,
            arg5: *const ISC_UCHAR,
            arg6: ISC_EVENT_CALLBACK,
            arg7: *mut ::std::os::raw::c_void,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_rollback_retaining(arg1: *mut ISC_STATUS, arg2: *mut isc_tr_handle) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_rollback_transaction(arg1: *mut ISC_STATUS, arg2: *mut isc_tr_handle) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_start_multiple(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_tr_handle,
            arg3: ::std::os::raw::c_short,
            arg4: *mut ::std::os::raw::c_void,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_start_transaction(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_tr_handle,
            arg3: ::std::os::raw::c_short,
            ...
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn fb_disconnect_transaction(arg1: *mut ISC_STATUS, arg2: *mut isc_tr_handle)
            -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_sqlcode(arg1: *const ISC_STATUS) -> ISC_LONG;
    }
    extern "C" {
        pub fn isc_sqlcode_s(arg1: *const ISC_STATUS, arg2: *mut ISC_ULONG);
    }
    extern "C" {
        pub fn fb_sqlstate(arg1: *mut ::std::os::raw::c_char, arg2: *const ISC_STATUS);
    }
    extern "C" {
        pub fn isc_sql_interprete(
            arg1: ::std::os::raw::c_short,
            arg2: *mut ISC_SCHAR,
            arg3: ::std::os::raw::c_short,
        );
    }
    extern "C" {
        pub fn isc_transaction_info(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_tr_handle,
            arg3: ::std::os::raw::c_short,
            arg4: *const ISC_SCHAR,
            arg5: ::std::os::raw::c_short,
            arg6: *mut ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_transact_request(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: ::std::os::raw::c_ushort,
            arg5: *mut ISC_SCHAR,
            arg6: ::std::os::raw::c_ushort,
            arg7: *mut ISC_SCHAR,
            arg8: ::std::os::raw::c_ushort,
            arg9: *mut ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_vax_integer(arg1: *const ISC_SCHAR, arg2: ::std::os::raw::c_short) -> ISC_LONG;
    }
    extern "C" {
        pub fn isc_portable_integer(arg1: *const ISC_UCHAR, arg2: ::std::os::raw::c_short)
            -> ISC_INT64;
    }
    extern "C" {
        pub fn isc_add_user(arg1: *mut ISC_STATUS, arg2: *const USER_SEC_DATA) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_delete_user(arg1: *mut ISC_STATUS, arg2: *const USER_SEC_DATA) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_modify_user(arg1: *mut ISC_STATUS, arg2: *const USER_SEC_DATA) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_compile_request(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_req_handle,
            arg4: ::std::os::raw::c_short,
            arg5: *const ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_compile_request2(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_req_handle,
            arg4: ::std::os::raw::c_short,
            arg5: *const ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_ddl(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: ::std::os::raw::c_short,
            arg5: *const ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_prepare_transaction(arg1: *mut ISC_STATUS, arg2: *mut isc_tr_handle) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_receive(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_req_handle,
            arg3: ::std::os::raw::c_short,
            arg4: ::std::os::raw::c_short,
            arg5: *mut ::std::os::raw::c_void,
            arg6: ::std::os::raw::c_short,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_reconnect_transaction(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: ::std::os::raw::c_short,
            arg5: *const ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_release_request(arg1: *mut ISC_STATUS, arg2: *mut isc_req_handle) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_request_info(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_req_handle,
            arg3: ::std::os::raw::c_short,
            arg4: ::std::os::raw::c_short,
            arg5: *const ISC_SCHAR,
            arg6: ::std::os::raw::c_short,
            arg7: *mut ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_seek_blob(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_blob_handle,
            arg3: ::std::os::raw::c_short,
            arg4: ISC_LONG,
            arg5: *mut ISC_LONG,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_send(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_req_handle,
            arg3: ::std::os::raw::c_short,
            arg4: ::std::os::raw::c_short,
            arg5: *const ::std::os::raw::c_void,
            arg6: ::std::os::raw::c_short,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_start_and_send(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_req_handle,
            arg3: *mut isc_tr_handle,
            arg4: ::std::os::raw::c_short,
            arg5: ::std::os::raw::c_short,
            arg6: *const ::std::os::raw::c_void,
            arg7: ::std::os::raw::c_short,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_start_request(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_req_handle,
            arg3: *mut isc_tr_handle,
            arg4: ::std::os::raw::c_short,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_unwind_request(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_tr_handle,
            arg3: ::std::os::raw::c_short,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_wait_for_event(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: ::std::os::raw::c_short,
            arg4: *const ISC_UCHAR,
            arg5: *mut ISC_UCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_close(arg1: *mut ISC_STATUS, arg2: *const ISC_SCHAR) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_declare(
            arg1: *mut ISC_STATUS,
            arg2: *const ISC_SCHAR,
            arg3: *const ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_describe(
            arg1: *mut ISC_STATUS,
            arg2: *const ISC_SCHAR,
            arg3: *mut XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_describe_bind(
            arg1: *mut ISC_STATUS,
            arg2: *const ISC_SCHAR,
            arg3: *mut XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_execute(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_tr_handle,
            arg3: *const ISC_SCHAR,
            arg4: *mut XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_execute_immediate(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: *mut ::std::os::raw::c_short,
            arg5: *const ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_fetch(
            arg1: *mut ISC_STATUS,
            arg2: *const ISC_SCHAR,
            arg3: *mut XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_open(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_tr_handle,
            arg3: *const ISC_SCHAR,
            arg4: *mut XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_prepare(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: *const ISC_SCHAR,
            arg5: *const ::std::os::raw::c_short,
            arg6: *const ISC_SCHAR,
            arg7: *mut XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_execute_m(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_tr_handle,
            arg3: *mut isc_stmt_handle,
            arg4: ::std::os::raw::c_ushort,
            arg5: *const ISC_SCHAR,
            arg6: ::std::os::raw::c_ushort,
            arg7: ::std::os::raw::c_ushort,
            arg8: *mut ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_execute2_m(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_tr_handle,
            arg3: *mut isc_stmt_handle,
            arg4: ::std::os::raw::c_ushort,
            arg5: *const ISC_SCHAR,
            arg6: ::std::os::raw::c_ushort,
            arg7: ::std::os::raw::c_ushort,
            arg8: *mut ISC_SCHAR,
            arg9: ::std::os::raw::c_ushort,
            arg10: *mut ISC_SCHAR,
            arg11: ::std::os::raw::c_ushort,
            arg12: ::std::os::raw::c_ushort,
            arg13: *mut ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_execute_immediate_m(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: ::std::os::raw::c_ushort,
            arg5: *const ISC_SCHAR,
            arg6: ::std::os::raw::c_ushort,
            arg7: ::std::os::raw::c_ushort,
            arg8: *mut ISC_SCHAR,
            arg9: ::std::os::raw::c_ushort,
            arg10: ::std::os::raw::c_ushort,
            arg11: *mut ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_exec_immed3_m(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: ::std::os::raw::c_ushort,
            arg5: *const ISC_SCHAR,
            arg6: ::std::os::raw::c_ushort,
            arg7: ::std::os::raw::c_ushort,
            arg8: *mut ISC_SCHAR,
            arg9: ::std::os::raw::c_ushort,
            arg10: ::std::os::raw::c_ushort,
            arg11: *const ISC_SCHAR,
            arg12: ::std::os::raw::c_ushort,
            arg13: *mut ISC_SCHAR,
            arg14: ::std::os::raw::c_ushort,
            arg15: ::std::os::raw::c_ushort,
            arg16: *mut ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_fetch_m(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_stmt_handle,
            arg3: ::std::os::raw::c_ushort,
            arg4: *mut ISC_SCHAR,
            arg5: ::std::os::raw::c_ushort,
            arg6: ::std::os::raw::c_ushort,
            arg7: *mut ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_insert_m(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_stmt_handle,
            arg3: ::std::os::raw::c_ushort,
            arg4: *const ISC_SCHAR,
            arg5: ::std::os::raw::c_ushort,
            arg6: ::std::os::raw::c_ushort,
            arg7: *const ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_prepare_m(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_tr_handle,
            arg3: *mut isc_stmt_handle,
            arg4: ::std::os::raw::c_ushort,
            arg5: *const ISC_SCHAR,
            arg6: ::std::os::raw::c_ushort,
            arg7: ::std::os::raw::c_ushort,
            arg8: *const ISC_SCHAR,
            arg9: ::std::os::raw::c_ushort,
            arg10: *mut ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_dsql_release(arg1: *mut ISC_STATUS, arg2: *const ISC_SCHAR) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_embed_dsql_close(arg1: *mut ISC_STATUS, arg2: *const ISC_SCHAR) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_embed_dsql_declare(
            arg1: *mut ISC_STATUS,
            arg2: *const ISC_SCHAR,
            arg3: *const ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_embed_dsql_describe(
            arg1: *mut ISC_STATUS,
            arg2: *const ISC_SCHAR,
            arg3: ::std::os::raw::c_ushort,
            arg4: *mut XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_embed_dsql_describe_bind(
            arg1: *mut ISC_STATUS,
            arg2: *const ISC_SCHAR,
            arg3: ::std::os::raw::c_ushort,
            arg4: *mut XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_embed_dsql_execute(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_tr_handle,
            arg3: *const ISC_SCHAR,
            arg4: ::std::os::raw::c_ushort,
            arg5: *mut XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_embed_dsql_execute2(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_tr_handle,
            arg3: *const ISC_SCHAR,
            arg4: ::std::os::raw::c_ushort,
            arg5: *mut XSQLDA,
            arg6: *mut XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_embed_dsql_execute_immed(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: ::std::os::raw::c_ushort,
            arg5: *const ISC_SCHAR,
            arg6: ::std::os::raw::c_ushort,
            arg7: *mut XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_embed_dsql_fetch(
            arg1: *mut ISC_STATUS,
            arg2: *const ISC_SCHAR,
            arg3: ::std::os::raw::c_ushort,
            arg4: *mut XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_embed_dsql_fetch_a(
            arg1: *mut ISC_STATUS,
            arg2: *mut ::std::os::raw::c_int,
            arg3: *const ISC_SCHAR,
            arg4: ISC_USHORT,
            arg5: *mut XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_embed_dsql_length(arg1: *const ISC_UCHAR, arg2: *mut ISC_USHORT);
    }
    extern "C" {
        pub fn isc_embed_dsql_open(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_tr_handle,
            arg3: *const ISC_SCHAR,
            arg4: ::std::os::raw::c_ushort,
            arg5: *mut XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_embed_dsql_open2(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_tr_handle,
            arg3: *const ISC_SCHAR,
            arg4: ::std::os::raw::c_ushort,
            arg5: *mut XSQLDA,
            arg6: *mut XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_embed_dsql_insert(
            arg1: *mut ISC_STATUS,
            arg2: *const ISC_SCHAR,
            arg3: ::std::os::raw::c_ushort,
            arg4: *mut XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_embed_dsql_prepare(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut isc_tr_handle,
            arg4: *const ISC_SCHAR,
            arg5: ::std::os::raw::c_ushort,
            arg6: *const ISC_SCHAR,
            arg7: ::std::os::raw::c_ushort,
            arg8: *mut XSQLDA,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_embed_dsql_release(arg1: *mut ISC_STATUS, arg2: *const ISC_SCHAR) -> ISC_STATUS;
    }
    extern "C" {
        pub fn BLOB_open(
            arg1: isc_blob_handle,
            arg2: *mut ISC_SCHAR,
            arg3: ::std::os::raw::c_int,
        ) -> FB_BLOB_STREAM;
    }
    extern "C" {
        pub fn BLOB_put(arg1: ISC_SCHAR, arg2: FB_BLOB_STREAM) -> ::std::os::raw::c_int;
    }
    extern "C" {
        pub fn BLOB_close(arg1: FB_BLOB_STREAM) -> ::std::os::raw::c_int;
    }
    extern "C" {
        pub fn BLOB_get(arg1: FB_BLOB_STREAM) -> ::std::os::raw::c_int;
    }
    extern "C" {
        pub fn BLOB_display(
            arg1: *mut ISC_QUAD,
            arg2: isc_db_handle,
            arg3: isc_tr_handle,
            arg4: *const ISC_SCHAR,
        ) -> ::std::os::raw::c_int;
    }
    extern "C" {
        pub fn BLOB_dump(
            arg1: *mut ISC_QUAD,
            arg2: isc_db_handle,
            arg3: isc_tr_handle,
            arg4: *const ISC_SCHAR,
        ) -> ::std::os::raw::c_int;
    }
    extern "C" {
        pub fn BLOB_edit(
            arg1: *mut ISC_QUAD,
            arg2: isc_db_handle,
            arg3: isc_tr_handle,
            arg4: *const ISC_SCHAR,
        ) -> ::std::os::raw::c_int;
    }
    extern "C" {
        pub fn BLOB_load(
            arg1: *mut ISC_QUAD,
            arg2: isc_db_handle,
            arg3: isc_tr_handle,
            arg4: *const ISC_SCHAR,
        ) -> ::std::os::raw::c_int;
    }
    extern "C" {
        pub fn BLOB_text_dump(
            arg1: *mut ISC_QUAD,
            arg2: isc_db_handle,
            arg3: isc_tr_handle,
            arg4: *const ISC_SCHAR,
        ) -> ::std::os::raw::c_int;
    }
    extern "C" {
        pub fn BLOB_text_load(
            arg1: *mut ISC_QUAD,
            arg2: isc_db_handle,
            arg3: isc_tr_handle,
            arg4: *const ISC_SCHAR,
        ) -> ::std::os::raw::c_int;
    }
    extern "C" {
        pub fn Bopen(
            arg1: *mut ISC_QUAD,
            arg2: isc_db_handle,
            arg3: isc_tr_handle,
            arg4: *const ISC_SCHAR,
        ) -> FB_BLOB_STREAM;
    }
    extern "C" {
        pub fn isc_ftof(
            arg1: *const ISC_SCHAR,
            arg2: ::std::os::raw::c_ushort,
            arg3: *mut ISC_SCHAR,
            arg4: ::std::os::raw::c_ushort,
        ) -> ISC_LONG;
    }
    extern "C" {
        pub fn isc_print_blr(
            arg1: *const ISC_SCHAR,
            arg2: ISC_PRINT_CALLBACK,
            arg3: *mut ::std::os::raw::c_void,
            arg4: ::std::os::raw::c_short,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn fb_print_blr(
            arg1: *const ISC_UCHAR,
            arg2: ISC_ULONG,
            arg3: ISC_PRINT_CALLBACK,
            arg4: *mut ::std::os::raw::c_void,
            arg5: ::std::os::raw::c_short,
        ) -> ::std::os::raw::c_int;
    }
    extern "C" {
        pub fn isc_set_debug(arg1: ::std::os::raw::c_int);
    }
    extern "C" {
        pub fn isc_qtoq(arg1: *const ISC_QUAD, arg2: *mut ISC_QUAD);
    }
    extern "C" {
        pub fn isc_vtof(arg1: *const ISC_SCHAR, arg2: *mut ISC_SCHAR, arg3: ::std::os::raw::c_ushort);
    }
    extern "C" {
        pub fn isc_vtov(arg1: *const ISC_SCHAR, arg2: *mut ISC_SCHAR, arg3: ::std::os::raw::c_short);
    }
    extern "C" {
        pub fn isc_version(
            arg1: *mut isc_db_handle,
            arg2: ISC_VERSION_CALLBACK,
            arg3: *mut ::std::os::raw::c_void,
        ) -> ::std::os::raw::c_int;
    }
    extern "C" {
        pub fn isc_reset_fpe(arg1: ISC_USHORT) -> ISC_LONG;
    }
    extern "C" {
        pub fn isc_baddress(arg1: *mut ISC_SCHAR) -> usize;
    }
    extern "C" {
        pub fn isc_baddress_s(arg1: *const ISC_SCHAR, arg2: *mut usize);
    }
    extern "C" {
        pub fn isc_service_attach(
            arg1: *mut ISC_STATUS,
            arg2: ::std::os::raw::c_ushort,
            arg3: *const ISC_SCHAR,
            arg4: *mut isc_svc_handle,
            arg5: ::std::os::raw::c_ushort,
            arg6: *const ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_service_detach(arg1: *mut ISC_STATUS, arg2: *mut isc_svc_handle) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_service_query(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_svc_handle,
            arg3: *mut isc_resv_handle,
            arg4: ::std::os::raw::c_ushort,
            arg5: *const ISC_SCHAR,
            arg6: ::std::os::raw::c_ushort,
            arg7: *const ISC_SCHAR,
            arg8: ::std::os::raw::c_ushort,
            arg9: *mut ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_service_start(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_svc_handle,
            arg3: *mut isc_resv_handle,
            arg4: ::std::os::raw::c_ushort,
            arg5: *const ISC_SCHAR,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn fb_shutdown(
            arg1: ::std::os::raw::c_uint,
            arg2: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int;
    }
    extern "C" {
        pub fn fb_shutdown_callback(
            arg1: *mut ISC_STATUS,
            arg2: FB_SHUTDOWN_CALLBACK,
            arg3: ::std::os::raw::c_int,
            arg4: *mut ::std::os::raw::c_void,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn fb_cancel_operation(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: ISC_USHORT,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn fb_ping(arg1: *mut ISC_STATUS, arg2: *mut isc_db_handle) -> ISC_STATUS;
    }
    extern "C" {
        pub fn fb_get_database_handle(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_db_handle,
            arg3: *mut ::std::os::raw::c_void,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn fb_get_transaction_handle(
            arg1: *mut ISC_STATUS,
            arg2: *mut isc_tr_handle,
            arg3: *mut ::std::os::raw::c_void,
        ) -> ISC_STATUS;
    }
    extern "C" {
        pub fn isc_get_client_version(arg1: *mut ISC_SCHAR);
    }
    extern "C" {
        pub fn isc_get_client_major_version() -> ::std::os::raw::c_int;
    }
    extern "C" {
        pub fn isc_get_client_minor_version() -> ::std::os::raw::c_int;
    }
    extern "C" {
        pub fn fb_database_crypt_callback(
            arg1: *mut ISC_STATUS,
            arg2: *mut ::std::os::raw::c_void,
        ) -> ISC_STATUS;
    }
}

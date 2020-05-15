/*
 * Tests using the c api 
 *
 * gcc/clang use_c_api.c -lfbclient -o use    
 */

#include <time.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <ibase.h>

void insert_simple(isc_tr_handle *tr, isc_db_handle *conn);
void insert(isc_tr_handle *tr, isc_db_handle *conn);
void query(isc_tr_handle *tr, isc_db_handle *conn);

int main (int argc, char** argv) {

    ISC_STATUS_ARRAY status;
    isc_db_handle conn = 0;

    char host_db[] = "localhost:employe2.fdb";
    char login[] = "SYSDBA";
    char pass[] = "masterkey";
    
    short dpb_length = (short) (1 + strlen(login) + 2 + strlen(pass) + 2);
    char *dpb = (char *) malloc(dpb_length);
    
    *dpb = isc_dpb_version1;
    dpb_length = 1;
	isc_modify_dpb(&dpb, &dpb_length, isc_dpb_user_name, login, strlen(login));
	isc_modify_dpb(&dpb, &dpb_length, isc_dpb_password, pass, strlen(pass));

    if (isc_attach_database(status, 0, host_db, &conn, dpb_length, dpb)) {
	    fprintf(stderr, "%d - ", isc_sqlcode(status));
	    isc_print_status(status);
        return 1;
	}

    isc_tr_handle tr = 0;
    isc_start_transaction(status, &tr, 1, &conn, 0, NULL);

    printf("Insert...\n");
    insert_simple(&tr, &conn);
    printf("Insert...OK\n");

    printf("Insert2...\n");
    insert(&tr, &conn);
    printf("Insert2...OK\n");

    printf("Query...\n");
    query(&tr, &conn);
    printf("Query...OK\n");

    isc_commit_transaction(status, &tr);
    
    isc_detach_database(status, &conn);
}

typedef	struct
{
	short vary_length;
	char  vary_string[1];
} VARY;

void query(isc_tr_handle *tr, isc_db_handle *conn) {
    ISC_STATUS_ARRAY status;

    isc_stmt_handle stmt = 0;

    if (isc_dsql_alloc_statement2(status, conn, &stmt)) {
        fprintf(stderr, "on allocate: ");
		isc_print_sqlerror(isc_sqlcode(status), status);
        return;
    }

    XSQLDA *sqlda = (XSQLDA *) malloc(XSQLDA_LENGTH(4));
    sqlda->version = 1;
    sqlda->sqld = 4;
    
    char sql[] = "select from_currency, to_currency, conv_rate, update_date from cross_rate";

    if (isc_dsql_prepare(status, tr, &stmt, 0, sql, 3, sqlda)) {
        fprintf(stderr, "on prepare: ");
	    isc_print_sqlerror(isc_sqlcode(status), status);
        return;
    }

    if (isc_dsql_describe(status, &stmt, 1, sqlda)) {
        fprintf(stderr, "on describe: ");
        isc_print_sqlerror(isc_sqlcode(status), status);
        return;
    }

    short flags[] = {};

	for (int i = 0; i < sqlda->sqld; i++) {
        XSQLVAR *col = &sqlda->sqlvar[i];

        char *buffer;
		col->sqldata = (char*) malloc(col->sqllen);
        flags[i] = 0;
		col->sqlind = &flags[i];

        printf("[%-18s] | ", col->sqlname);
	}
    printf("\n");

    if (isc_dsql_execute(status, tr, &stmt, 3, NULL)) {
        fprintf(stderr, "on execute: ");
		isc_print_sqlerror(isc_sqlcode(status), status);
        return;
    }

    long fetch_stat;
    VARY *vary;
    struct tm tms;

    while ((fetch_stat = isc_dsql_fetch(status, &stmt, 1, sqlda)) == 0) {
        for (int i = 0; i < sqlda->sqld; i++) {
            XSQLVAR *col = &sqlda->sqlvar[i];
            
            if (*col->sqlind < 0) {
                printf("%-20s | ", "(null)");
            } else {
                switch (col->sqltype & ~1) {
                    case SQL_VARYING:
                        vary = (VARY*)col->sqldata;
                        printf("%-20s | ", vary->vary_string);
                        break;
                    case SQL_FLOAT:
                        printf("%-20f | ", (double)*(float*) col->sqldata);
                        break;
                    case SQL_TYPE_DATE:
                        isc_decode_sql_date((ISC_DATE *) col->sqldata, &tms);
                        char btms[16];
                        strftime(btms, sizeof(btms), "%d/%m/%Y", &tms);
                        printf("%-20s | ", btms);
                        break;
                    default:
                        printf("%-20s | ", "(none)");
                        break;
                }
            }

        }
        printf("\n");
    }

    if (fetch_stat != 100L) {
        fprintf(stderr, "on fetch: ");
		isc_print_sqlerror(isc_sqlcode(status), status);
        return;
    }

    if (isc_dsql_free_statement(status, &stmt, DSQL_close)) {
        fprintf(stderr, "on free: ");
		isc_print_sqlerror(isc_sqlcode(status), status);
        return;
    }
}

void insert_simple(isc_tr_handle *tr, isc_db_handle *conn) {
    ISC_STATUS_ARRAY status;

    if (isc_dsql_execute_immediate(status, conn, tr, 0, "delete from cross_rate where from_currency = 'Dollar' and to_currency = 'Real'", 1, NULL)) {
	    fprintf(stderr, "%d - ", isc_sqlcode(status));
	    isc_print_status(status);
	}

    if (isc_dsql_execute_immediate(status, conn, tr, 0, "insert into cross_rate (from_currency, to_currency, conv_rate) values ('Dollar', 'Real', 10)", 1, NULL)) {
	    fprintf(stderr, "%d - ", isc_sqlcode(status));
	    isc_print_status(status);
	}
}

void insert(isc_tr_handle *tr, isc_db_handle *conn) {
    ISC_STATUS_ARRAY status;

    if (isc_dsql_execute_immediate(status, conn, tr, 0, "delete from cross_rate where from_currency = 'Euro' and to_currency = 'Real'", 1, NULL)) {
	    fprintf(stderr, "%d - ", isc_sqlcode(status));
	    isc_print_status(status);
	}

    isc_stmt_handle stmt = 0;

    if (isc_dsql_alloc_statement2(status, conn, &stmt)) {
        fprintf(stderr, "on allocate: ");
		isc_print_sqlerror(isc_sqlcode(status), status);
        return;
    }

    XSQLDA *sqlda = (XSQLDA *) malloc(XSQLDA_LENGTH(3));
    sqlda->version = 1;
    sqlda->sqln = 3;
    
    char sql[] = "insert into cross_rate (from_currency, to_currency, conv_rate) values (?, ?, ?)";

    if (isc_dsql_prepare(status, tr, &stmt, 0, sql, 3, sqlda)) {
        fprintf(stderr, "on prepare: ");
	    isc_print_sqlerror(isc_sqlcode(status), status);
        return;
    }

    if (isc_dsql_describe_bind(status, &stmt, 1, sqlda)) {
        fprintf(stderr, "on describe: ");
        isc_print_sqlerror(isc_sqlcode(status), status);
        return;
    }

    char *from_currency = "Euro";
    sqlda->sqlvar[0].sqltype = SQL_VARYING;
    sqlda->sqlvar[0].sqldata = (char*) malloc(sizeof(from_currency));
    VARY *vary = (VARY *) sqlda->sqlvar[0].sqldata;
    vary->vary_length = 4;
    memcpy(vary->vary_string, from_currency, 4);

    char *to_currency = "Real";
    sqlda->sqlvar[1].sqltype = SQL_VARYING;
    sqlda->sqlvar[1].sqldata = (char*) malloc(sizeof(to_currency));
    vary = (VARY *) sqlda->sqlvar[1].sqldata;
    vary->vary_length = 4;
    memcpy(vary->vary_string, to_currency, 4);

    double conv_rate = 0.5;
    sqlda->sqlvar[2].sqltype = SQL_FLOAT;
    sqlda->sqlvar[2].sqllen = sizeof(conv_rate);
    sqlda->sqlvar[2].sqldata = &conv_rate;

    if (isc_dsql_execute(status, tr, &stmt, 3, sqlda)) {
        fprintf(stderr, "on execute: ");
		isc_print_sqlerror(isc_sqlcode(status), status);
        return;
    }
}

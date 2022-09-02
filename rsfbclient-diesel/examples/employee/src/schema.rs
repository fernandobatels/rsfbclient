
use diesel::*;

#[derive(Queryable)]
#[diesel(table_name = job)]
pub struct Job {
    #[diesel(column_name = job_code)]
    pub code: String,
    #[diesel(column_name = job_title)]
    pub title: String,
    #[diesel(column_name = job_country)]
    pub country: String,
}

table! {
    job(job_code) {
        job_code -> Text,
        job_title -> Text,
        job_country -> Text,
    }
}

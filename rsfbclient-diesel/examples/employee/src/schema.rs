
use diesel::*;
use tabled::Tabled;

#[derive(Queryable, Tabled, Insertable)]
#[diesel(table_name = job)]
pub struct Job {
    #[diesel(column_name = job_code)]
    pub code: String,
    #[diesel(column_name = job_title)]
    pub title: String,
    #[diesel(column_name = job_country)]
    pub country: String,
    #[diesel(column_name = job_grade)]
    pub grade: i16,
    pub min_salary: f32,
    pub max_salary: f32,
}

table! {
    job(job_code) {
        job_code -> Text,
        job_title -> Text,
        job_country -> Text,
        job_grade -> Smallint,
        min_salary -> Float,
        max_salary -> Float,
    }
}

#[derive(AsChangeset)]
#[diesel(table_name = job)]
pub struct JobUpdate {
    #[diesel(column_name = job_title)]
    pub title: Option<String>,
    pub min_salary: Option<f32>,
    pub max_salary: Option<f32>,
}

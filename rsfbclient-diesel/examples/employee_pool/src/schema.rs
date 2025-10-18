use diesel::{ Queryable };

#[allow(dead_code)]
#[derive(Queryable)]
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

diesel::table! {
    job(job_code) {
        job_code -> Text,
        job_title -> Text,
        job_country -> Text,
        job_grade -> Smallint,
        min_salary -> Float,
        max_salary -> Float,
    }
}

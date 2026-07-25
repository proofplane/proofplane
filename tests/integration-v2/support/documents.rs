use axum_test::multipart::{MultipartForm, Part};

pub fn upload_form(bytes: &[u8], filename: &str) -> MultipartForm {
    MultipartForm::new().add_part(
        "file",
        Part::bytes(bytes.to_vec())
            .file_name(filename)
            .mime_type("text/plain"),
    )
}

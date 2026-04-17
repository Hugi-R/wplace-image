use std::io::Cursor;
use std::os::raw::{c_char, c_int};

use rusqlite::ffi;
use rusqlite::functions::FunctionFlags;
use rusqlite::types::{ToSqlOutput, Value};
use rusqlite::{Connection, Result};

use wimage::{DateHours, PalettedImage, TileHistory};

#[no_mangle]
pub unsafe extern "C" fn sqlite3_extension_init(
    db: *mut ffi::sqlite3,
    _pz_err_msg: *mut *mut c_char,
    p_api: *mut ffi::sqlite3_api_routines,
) -> c_int {
    match Connection::extension_init2(db, p_api) {
        Ok(mut conn) => {
            if let Err(_) = extension_init(&mut conn) {
                return 1;
            }
            0
        }
        Err(_) => 1,
    }
}

fn cast_error(e: anyhow::Error) -> rusqlite::Error {
    rusqlite::Error::UserFunctionError(e.to_string().into())
}

fn extension_init(db: &mut Connection) -> Result<()> {
    db.create_scalar_function(
        "wimage_to_png",
        2,
        FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let compressed_bytes = ctx
                .get::<Vec<u8>>(0)?;
            let keep_diff = ctx
                .get::<i64>(1)?
                != 0;

            let paletted_image = PalettedImage::from_compressed_bytes(&compressed_bytes)
                .map_err(cast_error)?;

            let png_data = if keep_diff {
                paletted_image
                    .to_png_diff()
                    .map_err(cast_error)?
            } else {
                paletted_image
                    .to_png()
                    .map_err(cast_error)?
            };

            Ok(ToSqlOutput::Owned(Value::Blob(png_data)))
        },
    )?;

    db.create_scalar_function(
        "wimage_from_png",
        1,
        FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let png_data = ctx
                .get::<Vec<u8>>(0)?;
            let paletted_image = PalettedImage::from_png(Cursor::new(png_data))
                .map_err(cast_error)?;
            let compressed_image = paletted_image
                .to_compressed_bytes()
                .map_err(cast_error)?;

            Ok(ToSqlOutput::Owned(Value::Blob(compressed_image.0)))
        },
    )?;

    db.create_scalar_function(
        "wimage_get",
        2,
        FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let tilehistory_data = ctx
                .get::<Vec<u8>>(0)?;
            let date_hours = ctx
                .get::<u32>(1)?;
            let compressed_image = TileHistory::raw_get(&tilehistory_data, DateHours(date_hours))
                .map_err(cast_error)?;

            Ok(ToSqlOutput::Owned(Value::Blob(compressed_image.0)))
        },
    )?;

    Ok(())
}

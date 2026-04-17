# wimage loadable SQLite extension

use it like this:
```
.load target/debug/libsqlite_ext
select writefile('img.png', wimage_to_png(wimage_get(data, 0), false)) from tiles where z = 11 and x = 1 and y = 0;
```
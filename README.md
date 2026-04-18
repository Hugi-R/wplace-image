# wplace-image
A lib and tools to convert and process wplace.live images into my custom format.

See:
- [wimage](./wimage/): lib for image conversion and processing.
- [sqlite-ext](./sqlite-ext/): a SQLite loadable extension allowing to read the binary format from `wimage` directly in SQLite.

## Build
```shell
cargo build --release
```

## AI usage disclosure
AI may have been used on some part of the code for boilerplate or refactoring.
Most of the poor design choice are my fault alone.

## License
MIT
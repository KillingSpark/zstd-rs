# Changelog

This document records the changes made between versions, starting with version 0.5.0

# After 0.5.0
* Make the hashing checksum optional (thanks to [@tamird](https://github.com/tamird))
    * breaking change as the public API changes based on features
* The FrameDecoder is now Send + Sync (RingBuffer impls these traits now)

# After 0.6.0
* Small fix in the zstd binary, progress tracking was slighty off for skippable frames resulting in an error only when the last frame in a file was skippable
* Small performance improvement by reorganizing code with `#[cold]` annotations
* Documentation for `StreamDecoder` mentioning the limitations around multiple frames (https://github.com/Sorseg)
* Documentation around skippable frames (https://github.com/Sorseg)
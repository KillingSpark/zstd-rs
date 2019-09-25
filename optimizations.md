# Optimizations
This document tracks which optimizations have been done after the initial implementation passed corpus tests and a good amount of fuzzing.

## Introducing more unsafe code:
These optimizations introduced more unsafe code. These should yield significant improvements, or else they are not really worth it.

### Optimizing bitreader with byteorder which uses ptr::copy_nonoverlapping
* Reverse bitreader_reversed::get_bits was identified by linux perf tool using about 36% of the whole time
* Benchmark: decode enwik9

* Before: about 14.7 seconds
* After: about 12.2 seconds with about 25% of the time used for get_bits()

### Optimizing decodebuffer::repeat with ptr::copy_nonoverlapping
* decodebuffer::repeate was identified by linux perf tool using about 28% of the whole time
* Benchmark: decode enwik9

* Before: about 9.9 seconds
* After: about 9.4 seconds
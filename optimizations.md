# Introducing more unsafe code:

## Optimizing bitreader with byteorder which uses unsafe ptr::copy_nonoverlapping
* Reverse bitreader_reversed::get_bits was identified by linux perf tool using about 36% of the whole time
* Benchmark: decode enwik9

* Before: about 14.7 seconds
* After: about 12.2 seconds with about 25% of the time used for get_bits()
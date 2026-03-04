# rogue

A voxel engine that will be used for future games. It follows a similar architecture to other GUI based engines with a separate editor and runtime binary with project support, though dynamically linked game code will not be a thing for awhile.

![Image of editor.](docs/images/editor.png)

## Useful environment variables

`ROGUE_GFX_DEBUG=1` to enable Vulkan validation layers and graphics device error reporting.

`RUST_LOG=log_level` to change the current logs being displayed, usually log level is `info` or `debug`.

## Profiling/Debugging

I like to use [samply](https://github.com/mstange/samply) and its just `samply record target/debug/[binary]` and you get browser-based
flamegraph for cpu perf profiling. [heaptrack](https://github.com/KDE/heaptrack) is also nice for memory profiling like
seeing memory leaks and allocations. Should also use "--profile dev-noopt" when debugging so variables don't get optimized out and panic
messages have all line numbers.

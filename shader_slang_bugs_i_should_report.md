# Random bugs I find

These are my assumptions of what they are, i dont think its very helpful to report without a minimal testcase and idk I'll get to this if its not fixed already or reported.

### Matrix3x3 

matrix3x3 in a struct which is contained in a StructuredBuffer causes a spirv parsing issue.

```
[2025-12-02T23:11:55Z ERROR rogue::engine::graphics::vulkan::device] [general] "SPIR-V offset 64564: SPIR-V parsing FAILED:\n    Type mismatch for SPIR-V value %23791\n    64564 bytes into the SPIR-V binary"
[2025-12-02T23:11:55Z WARN  rogue::engine::graphics::vulkan::device] [general] "spirv_to_nir failed (VK_ERROR_UNKNOWN)"
```

and offending spirv code

darn I removed it but it was an OpCompositeExtract iirc of the matrix3x3, "solved" the bug
by just replacing with 3 vec 3s and constructing a matrix3x3 manually, so it must be related to matrix3x3.

### Sampler causes thread to not execute or something?

in material.slang when i use a sampler along with the texture, every 4 voxels in the y direction didnt have its material set properly, this would seem like a uv issue but even with a constant uv i had the same issue, doesnt occur when setting the color manually or doing a texel load from the texture, only using the sampler. didnt investigate the spirv and whats happening to the thread but probably not great?

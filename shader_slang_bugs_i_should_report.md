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

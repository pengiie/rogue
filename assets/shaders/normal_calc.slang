module normal_calc;

import lib;

struct ShaderUniforms {
  int3 chunk_min;
  int3 chunk_max;
}

ParameterBlock<ShaderUniforms> u_shader;

[shader("compute")]
[numthreads(8, 8, 8)]
void main(uint3 id: SV_DispatchThreadID) {
}

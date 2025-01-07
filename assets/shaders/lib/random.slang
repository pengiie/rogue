implementing lib;

namespace random {

static uint rng_state;

// A slightly modified version of the "One-at-a-Time Hash" function by Bob Jenkins.
// See https://www.burtleburtle.net/bob/hash/doobs.html
public uint jenkins_hash(uint i) {
  var x = i;
  x += x << 10u;
  x ^= x >> 6u;
  x += x << 3u;
  x ^= x >> 11u;
  x += x << 15u;
  return x;
}

public void init(uint3 xyz) {
  var n = 0x12341234u;
  n = (n<<13)^n; n=n*(n*n*15731+789221)+1376312589; // hash by Hugo Elias
  n += xyz.x;
  n = (n<<13)^n; n=n*(n*n*15731+789221)+1376312589;
  n += xyz.y;
  n = (n<<13)^n; n=n*(n*n*15731+789221)+1376312589;
  // Uncomment for temporal noise.
  n += jenkins_hash(xyz.z);
  n = (n<<13)^n; n=n*(n*n*15731+789221)+1376312589;

  rng_state = jenkins_hash(n);
}

// The 32-bit "xor" function from Marsaglia G., "Xorshift RNGs", Section 3.
public uint xorshift32() {
  var x = rng_state;
  x ^= x << 13;
  x ^= x >> 17;
  x ^= x << 5;
  rng_state = x;
  return x;
}

public uint rand_u32() {
  let x = xorshift32();
  return x;
}

public float rand_f32() {
  return reinterpret<float>(0x3f800000u | (rand_u32() >> 9u)) - 1.0;
}

} // namespace random

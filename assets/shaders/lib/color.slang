implementing lib;

public float3 srgb_to_lsrgb(float3 srgb) {
    let cutoff = srgb < float3(0.04045);
    let lower = srgb / float3(12.92);
    let higher = pow((srgb + float3(0.055)) / float3(1.055), float3(2.4));
    return select(cutoff, lower, higher);
}

public float3 lsrgb_to_srgb(float3 lsrgb) {
    let cutoff = lsrgb < float3(0.0031308);
    let lower = lsrgb * float3(12.92);
    let higher = float3(1.055) * pow(lsrgb, float3(1.0 / 2.4)) - float3(0.055);
    return select(cutoff, lower, higher);
}

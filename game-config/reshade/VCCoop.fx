// VCCoop.fx: one light pass for Vice City DE. Livelier colours (vibrance), a warm "neon sunset" touch,
// a soft S-curve for contrast and a gentle sharpen. Written for the SA/VC co-op mod; MIT.
#include "ReShade.fxh"

uniform float Vibrance < ui_type = "slider"; ui_min = 0.0; ui_max = 1.0; ui_label = "Vibrance"; > = 0.35;
uniform float Warmth < ui_type = "slider"; ui_min = -0.2; ui_max = 0.2; ui_label = "Warmth"; > = 0.05;
uniform float Contrast < ui_type = "slider"; ui_min = 0.0; ui_max = 1.0; ui_label = "Contrast (S-curve)"; > = 0.25;
uniform float Sharpen < ui_type = "slider"; ui_min = 0.0; ui_max = 1.5; ui_label = "Sharpen"; > = 0.5;

float3 VCCoopPS(float4 pos : SV_Position, float2 uv : TEXCOORD) : SV_Target
{
	float3 c = tex2D(ReShade::BackBuffer, uv).rgb;

	// sharpen: the pixel against the average of its four neighbours, clamped to avoid halos
	float2 px = BUFFER_PIXEL_SIZE;
	float3 blur = (tex2D(ReShade::BackBuffer, uv + float2(px.x, 0)).rgb + tex2D(ReShade::BackBuffer, uv - float2(px.x, 0)).rgb
	             + tex2D(ReShade::BackBuffer, uv + float2(0, px.y)).rgb + tex2D(ReShade::BackBuffer, uv - float2(0, px.y)).rgb) * 0.25;
	float edge = dot(c - blur, float3(0.299, 0.587, 0.114));
	c += clamp(edge * Sharpen, -0.05, 0.05);

	// vibrance: lift saturation most where colour is weak
	float luma = dot(c, float3(0.299, 0.587, 0.114));
	float sat = max(c.r, max(c.g, c.b)) - min(c.r, min(c.g, c.b));
	c = lerp(luma.xxx, c, 1.0 + Vibrance * (1.0 - sat));

	// warmth: a little red up and blue down
	c *= float3(1.0 + Warmth, 1.0, 1.0 - Warmth);

	// contrast: smoothstep S-curve blended in
	float3 s = c * c * (3.0 - 2.0 * c);
	c = lerp(c, s, Contrast);

	return saturate(c);
}

technique VCCoop < ui_label = "VC Co-op look"; ui_tooltip = "Vibrance, warmth, contrast, sharpen"; >
{
	pass
	{
		VertexShader = PostProcessVS;
		PixelShader = VCCoopPS;
	}
}

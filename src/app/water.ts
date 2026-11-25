// 换配色的招牌时刻（palette-lake 1.2，`decisions/0033`）：整页像水面一样扭，新颜色从点的地方像墨一样洇开。
// WebKit 的 SVG 滤镜在 CPU 上算（整页每帧约 100 毫秒），所以这里换个做法：
//   1. 让 App 拍一张网页当前的样子（Rust 的 app_snapshot，WKWebView 的 takeSnapshot，原始像素）；
//   2. 盖一张 WebGL 画布在最上面，先原样画出旧样子；底下换配色（样式只重算一次，被画布挡着）；
//   3. 着色器在 GPU 上逐像素扭：五圈波纹按方案的节奏折射（和上面那层 SVG 波纹同一套半径）、整页一层慢慢晃的水波，
//      洇开的圈里画布变透明、露出底下真的新样子；1.6 秒后拿掉画布。
// 拍不到（浏览器里预览、不是 macOS）、没有 WebGL 时返回 false，换回 View Transitions 的洇开（shared/theme.ts）。
import { api } from "@/shared/ipc";

type At = { x: number; y: number };

// 波纹：和 palette-lake.css 的五圈一样（延时、放大倍数、2.1 秒、cubic-bezier(.15,.65,.3,1)；圈的半径是 10px × 倍数）
const RINGS = [
  { d: 0, k: 62, amp: 9 },
  { d: 0.16, k: 52, amp: 7.5 },
  { d: 0.34, k: 42, amp: 6 },
  { d: 0.54, k: 32, amp: 4.5 },
  { d: 0.76, k: 22, amp: 3.5 },
];
const RING_DUR = 2.1;
const SWELL = { keys: [0, 0.22, 0.5, 0.75, 1], vals: [0, 5.5, 2.5, 1, 0], dur: 1.4 }; // 整页的晃：和原来滤镜的 0;11;5;2;0 同一个节奏
const INK = { delay: 0.12, dur: 1.45, soft: 56, ragged: 90 }; // 洇开跟在波纹后面：先晃，颜色再慢慢洇过去；边缘窄、毛（软边太宽，扭过的旧字和新字会叠出重影）
const TOTAL = 1.65;

/** CSS 的 cubic-bezier(x1, y1, x2, y2)：给时间的比例，返回进度 */
function bezier(x1: number, y1: number, x2: number, y2: number) {
  const cx = 3 * x1, bx = 3 * (x2 - x1) - cx, ax = 1 - cx - bx;
  const cy = 3 * y1, by = 3 * (y2 - y1) - cy, ay = 1 - cy - by;
  const x = (t: number) => ((ax * t + bx) * t + cx) * t;
  const y = (t: number) => ((ay * t + by) * t + cy) * t;
  return (p: number) => {
    if (p <= 0) return 0;
    if (p >= 1) return 1;
    let t = p;
    for (let i = 0; i < 8; i++) {
      const dx = x(t) - p;
      const d = (3 * ax * t + 2 * bx) * t + cx;
      if (Math.abs(dx) < 1e-5 || Math.abs(d) < 1e-6) break;
      t -= dx / d;
    }
    return y(Math.min(1, Math.max(0, t)));
  };
}
const ringEase = bezier(0.15, 0.65, 0.3, 1);
const inkEase = bezier(0.4, 0.05, 0.25, 1);

const VERT = `#version 300 es
in vec2 aPos;
uniform vec2 uSize;
out vec2 vP;
void main() {
  vP = vec2(aPos.x * .5 + .5, .5 - aPos.y * .5) * uSize;
  gl_Position = vec4(aPos, 0., 1.);
}`;

const FRAG = `#version 300 es
precision highp float;
uniform sampler2D uTex;
uniform vec2 uSize;
uniform vec2 uC;
uniform float uDpr;
uniform float uR[5];
uniform float uA[5];
uniform float uW[5];
uniform float uSwell;
uniform float uT;
uniform float uInk;
uniform float uSoft;
uniform float uRagged;
uniform int uOrder;
in vec2 vP;
out vec4 o;
float hash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
float noise(vec2 p) {
  vec2 i = floor(p), f = fract(p), u = f * f * (3. - 2. * f);
  return mix(mix(hash(i), hash(i + vec2(1, 0)), u.x), mix(hash(i + vec2(0, 1)), hash(i + vec2(1, 1)), u.x), u.y);
}
void main() {
  vec2 d = vP - uC;
  float r = length(d);
  vec2 n = r > .5 ? d / r : vec2(0.);
  // 五圈波纹：高斯的导数，过圈的地方往外推、圈里往回收，像透过水看纸
  float w = 0., slope = 0.;
  for (int i = 0; i < 5; i++) {
    float x = (r - uR[i]) / uW[i];
    float g = exp(-x * x);
    w += uA[i] * x * g;
    slope += uA[i] * (1. - 2. * x * x) * g / uW[i];
  }
  // 整页的水波：两层低频的噪声，慢慢漂
  vec2 cp = vP / uDpr;
  vec2 s = vec2(noise(cp * vec2(.011, .016) + vec2(uT * .35, 0.)), noise(cp * vec2(.016, .011) + vec2(17., uT * .3))) - .5;
  s += .5 * (vec2(noise(cp * vec2(.024, .034) + 31.), noise(cp * vec2(.034, .024) + 7. + uT * .2)) - .5);
  vec2 q = vP - n * w * uDpr - s * uSwell * 2. * uDpr;
  vec4 c = texture(uTex, clamp(q / uSize, vec2(0.), vec2(1.)));
  if (uOrder == 1) c = c.bgra; else if (uOrder == 2) c = c.gbar;
  // 波峰亮一点、波谷暗一点（很轻：奶油纸提一点就成了白）
  c.rgb *= 1. + clamp(slope * .45, -.025, .025);
  // 洇开：圈里透明（露出底下的新样子），边缘是一圈窄的、毛的墨，像墨在湿纸上走
  float e = r + ((noise(cp * .016 + 5.) - .5) + .45 * (noise(cp * .05 + 11.) - .5)) * uRagged * uDpr;
  float a = smoothstep(uInk - uSoft, uInk, e);
  o = vec4(c.rgb * a, a);
}`;

let gl: WebGL2RenderingContext | null = null;
let canvas: HTMLCanvasElement | null = null;
let prog: WebGLProgram | null = null;
let tex: WebGLTexture | null = null;

function setup(): boolean {
  if (gl) return true;
  canvas = document.createElement("canvas");
  canvas.className = "water-layer";
  canvas.setAttribute("aria-hidden", "true");
  const c = canvas.getContext("webgl2", { alpha: true, premultipliedAlpha: true, antialias: false, depth: false, stencil: false });
  if (!c) return false;
  const sh = (type: number, src: string) => {
    const s = c.createShader(type)!;
    c.shaderSource(s, src);
    c.compileShader(s);
    if (!c.getShaderParameter(s, c.COMPILE_STATUS)) throw new Error(c.getShaderInfoLog(s) ?? "着色器编译失败");
    return s;
  };
  const p = c.createProgram()!;
  c.attachShader(p, sh(c.VERTEX_SHADER, VERT));
  c.attachShader(p, sh(c.FRAGMENT_SHADER, FRAG));
  c.linkProgram(p);
  if (!c.getProgramParameter(p, c.LINK_STATUS)) throw new Error(c.getProgramInfoLog(p) ?? "着色器链接失败");
  c.useProgram(p);
  const buf = c.createBuffer();
  c.bindBuffer(c.ARRAY_BUFFER, buf);
  c.bufferData(c.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), c.STATIC_DRAW); // 一个盖满屏的三角形
  const loc = c.getAttribLocation(p, "aPos");
  c.enableVertexAttribArray(loc);
  c.vertexAttribPointer(loc, 2, c.FLOAT, false, 0, 0);
  tex = c.createTexture();
  c.bindTexture(c.TEXTURE_2D, tex);
  c.texParameteri(c.TEXTURE_2D, c.TEXTURE_MIN_FILTER, c.LINEAR);
  c.texParameteri(c.TEXTURE_2D, c.TEXTURE_MAG_FILTER, c.LINEAR);
  c.texParameteri(c.TEXTURE_2D, c.TEXTURE_WRAP_S, c.CLAMP_TO_EDGE);
  c.texParameteri(c.TEXTURE_2D, c.TEXTURE_WRAP_T, c.CLAMP_TO_EDGE);
  c.clearColor(0, 0, 0, 0);
  gl = c;
  prog = p;
  return true;
}

const frame = () => new Promise<number>((r) => requestAnimationFrame(r));

let warmed = false;
/** 预热：先编好着色器、空拍一张（WebKit 第一次截图要 0.3 秒左右，之后 50 毫秒）。外观页、⌘K 打开时调，闲下来才做 */
export function warmWater() {
  if (warmed || !waterAvailable()) return;
  warmed = true;
  const go = () => {
    try {
      if (setup()) void api.appSnapshot().catch(() => {});
    } catch {
      // 预热失败不要紧，换的时候再试
    }
  };
  if ("requestIdleCallback" in window) (window as unknown as { requestIdleCallback: (f: () => void, o: { timeout: number }) => void }).requestIdleCallback(go, { timeout: 1200 });
  else setTimeout(go, 400);
}

/** 能不能走这条路：在 App 里（有 Tauri）、有 WebGL2 */
export function waterAvailable(): boolean {
  return "__TAURI_INTERNALS__" in window && !new URLSearchParams(location.search).has("mock") && typeof WebGL2RenderingContext !== "undefined";
}

/**
 * 演一次：swap 在画布盖好之后调（底下换配色），ready 在真正开始动的那一刻调（上面那层 SVG 波纹跟着起）。
 * 返回 false：没演（拍不到、没有 WebGL），swap 也没调，调用方自己换。
 */
export async function playWater(at: At, swap: () => void, ready: () => void): Promise<boolean> {
  let shot: ArrayBuffer;
  const t0 = performance.now();
  try {
    if (!setup()) return false;
    shot = await api.appSnapshot();
  } catch {
    return false;
  }
  const tShot = performance.now();
  if (!(shot instanceof ArrayBuffer) || shot.byteLength < 12) return false;
  const head = new DataView(shot, 0, 12);
  const [w, h, order] = [head.getUint32(0, true), head.getUint32(4, true), head.getUint32(8, true)];
  if (shot.byteLength < 12 + w * h * 4) return false;
  const c = gl!, cv = canvas!, p = prog!;
  cv.width = w;
  cv.height = h;
  c.viewport(0, 0, w, h);
  c.bindTexture(c.TEXTURE_2D, tex);
  c.pixelStorei(c.UNPACK_ALIGNMENT, 4);
  c.texImage2D(c.TEXTURE_2D, 0, c.RGBA, w, h, 0, c.RGBA, c.UNSIGNED_BYTE, new Uint8Array(shot, 12, w * h * 4));
  const tUp = performance.now();
  const dpr = w / innerWidth;
  const u = (name: string) => c.getUniformLocation(p, name);
  c.uniform2f(u("uSize"), w, h);
  c.uniform2f(u("uC"), at.x * dpr, at.y * dpr);
  c.uniform1f(u("uDpr"), dpr);
  c.uniform1i(u("uOrder"), order);
  c.uniform1f(u("uSoft"), INK.soft * dpr);
  c.uniform1f(u("uRagged"), INK.ragged);
  const far = Math.hypot(Math.max(at.x, innerWidth - at.x), Math.max(at.y, innerHeight - at.y)) + INK.soft + INK.ragged;
  const R = new Float32Array(5), A = new Float32Array(5), W = new Float32Array(5);

  const draw = (t: number) => {
    RINGS.forEach((g, i) => {
      const q = Math.min(1, Math.max(0, (t - g.d) / RING_DUR));
      const radius = 10 * (0.15 + (g.k - 0.15) * ringEase(q));
      const env = t < g.d ? 0 : q < 0.08 ? ringEase(q / 0.08) : 1 - ringEase((q - 0.08) / 0.92);
      R[i] = radius * dpr;
      A[i] = g.amp * env;
      W[i] = (18 + 0.1 * radius) * dpr;
    });
    const sp = Math.min(1, t / SWELL.dur);
    let swell = 0;
    for (let k = 1; k < SWELL.keys.length; k++) {
      if (sp <= SWELL.keys[k]) {
        const f = (sp - SWELL.keys[k - 1]) / (SWELL.keys[k] - SWELL.keys[k - 1]);
        swell = SWELL.vals[k - 1] + (SWELL.vals[k] - SWELL.vals[k - 1]) * f;
        break;
      }
    }
    const ink = inkEase(Math.min(1, Math.max(0, (t - INK.delay) / INK.dur))) * far;
    c.uniform1fv(u("uR"), R);
    c.uniform1fv(u("uA"), A);
    c.uniform1fv(u("uW"), W);
    c.uniform1f(u("uSwell"), swell);
    c.uniform1f(u("uT"), t);
    c.uniform1f(u("uInk"), ink * dpr);
    c.clear(c.COLOR_BUFFER_BIT);
    c.drawArrays(c.TRIANGLES, 0, 3);
  };

  // 先原样盖上旧样子，等它真的显示出来，再在底下换
  document.body.append(cv);
  draw(0);
  await frame();
  await frame();
  try {
    swap();
  } finally {
    await frame(); // 新样子的样式在这一帧里算完
    ready();
    const tGo = performance.now();
    // 开发构建里记下各步花了多久（src/dev/perfProbe.ts 读）
    if (import.meta.env.DEV) (window as unknown as { __water?: unknown }).__water = { shot: Math.round(tShot - t0), upload: Math.round(tUp - tShot), start: Math.round(tGo - t0), w, h, order };
    await new Promise<void>((done) => {
      const tick = (now: number) => {
        const t = (now - tGo) / 1000;
        draw(t);
        if (t < TOTAL) requestAnimationFrame(tick);
        else done();
      };
      requestAnimationFrame(tick);
    });
    cv.remove();
  }
  return true;
}

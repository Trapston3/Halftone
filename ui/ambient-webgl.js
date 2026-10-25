/**
 * ui/ambient-webgl.js — Halftone v1.1 WebGL2 Liquid Aurora Ambient Shader
 *
 * Provides a 60fps GPU-accelerated liquid-mesh / aurora field for Digital mode:
 * - Domain-warped fractional Brownian motion (fBM) with chromatic refraction
 * - Smooth color interpolation matching album-art accent tokens (--accent, --accent-2)
 * - Spectrum audio-reactivity (bass swell & energy pulse)
 * - High-frequency dither grain to completely eliminate 8-bit color banding
 * - Visibility & reduced-motion auto-hibernation to preserve battery and GPU cycles
 */
(function () {
  "use strict";

  const VERT_SRC = `
    attribute vec2 a_pos;
    void main() {
      gl_Position = vec4(a_pos, 0.0, 1.0);
    }
  `;

  const FRAG_SRC = `
    precision mediump float;
    uniform float u_time;
    uniform vec2 u_resolution;
    uniform vec3 u_color_primary;
    uniform vec3 u_color_secondary;
    uniform vec3 u_color_deep;
    uniform vec3 u_color_bg;
    uniform float u_audio_energy;
    uniform float u_audio_bass;
    uniform float u_is_dark;
    uniform float u_intensity;

    vec2 hash2(vec2 p) {
      p = vec2(dot(p, vec2(127.1, 311.7)), dot(p, vec2(269.5, 183.3)));
      return -1.0 + 2.0 * fract(sin(p) * 43758.5453123);
    }

    float noise(vec2 p) {
      vec2 i = floor(p);
      vec2 f = fract(p);
      vec2 u = f * f * (3.0 - 2.0 * f);
      return mix(mix(dot(hash2(i + vec2(0.0, 0.0)), f - vec2(0.0, 0.0)),
                     dot(hash2(i + vec2(1.0, 0.0)), f - vec2(1.0, 0.0)), u.x),
                 mix(dot(hash2(i + vec2(0.0, 1.0)), f - vec2(0.0, 1.0)),
                     dot(hash2(i + vec2(1.0, 1.0)), f - vec2(1.0, 1.0)), u.x), u.y);
    }

    float fbm(vec2 p) {
      float v = 0.0;
      float a = 0.5;
      mat2 rot = mat2(0.87758, 0.47942, -0.47942, 0.87758);
      for (int i = 0; i < 3; i++) {
        v += a * noise(p);
        p = rot * p * 2.02 + vec2(100.0);
        a *= 0.5;
      }
      return v;
    }

    void main() {
      vec2 uv = gl_FragCoord.xy / u_resolution.xy;
      vec2 p = (gl_FragCoord.xy * 2.0 - u_resolution.xy) / min(u_resolution.x, u_resolution.y);

      float t = u_time * 0.12;
      float bass = u_audio_bass * 0.32;

      vec2 q = vec2(
        fbm(p + vec2(0.0, 0.0) + t * 0.7),
        fbm(p + vec2(5.2, 1.3) + t * 0.5)
      );

      vec2 r = vec2(
        fbm(p + 3.0 * q + vec2(1.7, 9.2) + t * 0.4 + bass),
        fbm(p + 3.0 * q + vec2(8.3, 2.8) + t * 0.3)
      );

      float f = fbm(p + 3.2 * r);
      float fNorm = clamp((f + 0.5) * 0.9 + u_audio_energy * 0.22, 0.0, 1.0);

      float dist = length(uv - vec2(0.5, 0.38));
      float vignette = clamp(1.0 - dist * 0.85, 0.15, 1.0);

      vec3 col = u_color_bg;
      col = mix(col, u_color_deep, clamp(fNorm * 1.25, 0.0, 1.0));
      col = mix(col, u_color_secondary, clamp(length(q) * 1.15, 0.0, 1.0));
      col = mix(col, u_color_primary, clamp(r.x * r.x * 2.1 + u_audio_energy * 0.35, 0.0, 1.0));

      // Dithering grain to prevent color banding in dark gradients
      float dither = (fract(sin(dot(gl_FragCoord.xy, vec2(12.9898, 78.233))) * 43758.5453) - 0.5) * (1.6 / 255.0);
      col += dither;

      float baseAlpha = u_is_dark > 0.5 ? 0.72 : 0.46;
      float alpha = clamp(baseAlpha * u_intensity * vignette, 0.0, 1.0);
      gl_FragColor = vec4(col, alpha);
    }
  `;

  class HalftoneAmbientShader {
    constructor() {
      this.canvas = null;
      this.gl = null;
      this.program = null;
      this.uniforms = {};
      this.running = false;
      this.rafId = 0;
      this.startTime = performance.now();
      this.audioEnergy = 0;
      this.audioBass = 0;

      // Color targets & current interpolated values
      this.colors = {
        primary: [0.48, 0.42, 1.0],
        secondary: [0.18, 0.65, 0.98],
        deep: [0.06, 0.10, 0.22],
        bg: [0.02, 0.03, 0.05],
      };
      this.curColors = {
        primary: [...this.colors.primary],
        secondary: [...this.colors.secondary],
        deep: [...this.colors.deep],
        bg: [...this.colors.bg],
      };

      this.boundTick = this.tick.bind(this);
      this.boundResize = this.resize.bind(this);
      this.boundVisibility = this.onVisibilityChange.bind(this);
    }

    init(targetCanvas) {
      let cv = targetCanvas;
      if (!cv) {
        cv = document.querySelector("#ambientCanvas") ||
             document.querySelector("#wAmbient") ||
             document.querySelector(".fx-ambient");
      }
      if (!cv) {
        const layer = document.querySelector(".fx-layer") || document.body;
        cv = document.createElement("canvas");
        cv.id = "ambientCanvas";
        cv.className = "fx-ambient";
        layer.prepend(cv);
      }
      this.canvas = cv;

      const opts = { alpha: true, antialias: false, depth: false, powerPreference: "low-power" };
      let gl = cv.getContext("webgl2", opts);
      if (!gl) gl = cv.getContext("webgl", opts) || cv.getContext("experimental-webgl", opts);
      if (!gl) {
        console.warn("WebGL not supported for HalftoneAmbient");
        return false;
      }
      this.gl = gl;

      const prog = this.createProgram(VERT_SRC, FRAG_SRC);
      if (!prog) return false;
      this.program = prog;

      // Setup full-screen quad
      const posBuf = gl.createBuffer();
      gl.bindBuffer(gl.ARRAY_BUFFER, posBuf);
      gl.bufferData(
        gl.ARRAY_BUFFER,
        new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]),
        gl.STATIC_DRAW
      );
      const aPos = gl.getAttribLocation(prog, "a_pos");
      gl.enableVertexAttribArray(aPos);
      gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 0, 0);

      // Cache uniform locations
      const uNames = [
        "u_time", "u_resolution", "u_color_primary", "u_color_secondary",
        "u_color_deep", "u_color_bg", "u_audio_energy", "u_audio_bass",
        "u_is_dark", "u_intensity"
      ];
      for (const name of uNames) {
        this.uniforms[name] = gl.getUniformLocation(prog, name);
      }

      window.addEventListener("resize", this.boundResize, { passive: true });
      document.addEventListener("visibilitychange", this.boundVisibility);
      document.addEventListener("halftone:theme", () => this.updateColorsFromTokens());

      this.resize();
      this.updateColorsFromTokens();
      this.start();
      return true;
    }

    createProgram(vSrc, fSrc) {
      const gl = this.gl;
      const vShader = gl.createShader(gl.VERTEX_SHADER);
      gl.shaderSource(vShader, vSrc);
      gl.compileShader(vShader);
      if (!gl.getShaderParameter(vShader, gl.COMPILE_STATUS)) {
        console.warn("vShader err", gl.getShaderInfoLog(vShader));
        return null;
      }

      const fShader = gl.createShader(gl.FRAGMENT_SHADER);
      gl.shaderSource(fShader, fSrc);
      gl.compileShader(fShader);
      if (!gl.getShaderParameter(fShader, gl.COMPILE_STATUS)) {
        console.warn("fShader err", gl.getShaderInfoLog(fShader));
        return null;
      }

      const prog = gl.createProgram();
      gl.attachShader(prog, vShader);
      gl.attachShader(prog, fShader);
      gl.linkProgram(prog);
      if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
        console.warn("prog link err", gl.getProgramInfoLog(prog));
        return null;
      }
      return prog;
    }

    resize() {
      if (!this.canvas || !this.gl) return;
      const dpr = Math.min(window.devicePixelRatio || 1, 1.25);
      const w = Math.round((this.canvas.clientWidth || window.innerWidth) * dpr);
      const h = Math.round((this.canvas.clientHeight || window.innerHeight) * dpr);
      if (this.canvas.width !== w || this.canvas.height !== h) {
        this.canvas.width = Math.max(1, w);
        this.canvas.height = Math.max(1, h);
        this.gl.viewport(0, 0, this.canvas.width, this.canvas.height);
      }
    }

    parseRgbString(str) {
      if (!str) return null;
      const m = str.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/i);
      if (m) {
        return [parseInt(m[1], 10) / 255, parseInt(m[2], 10) / 255, parseInt(m[3], 10) / 255];
      }
      const hexM = str.match(/#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})/i);
      if (hexM) {
        return [parseInt(hexM[1], 16) / 255, parseInt(hexM[2], 16) / 255, parseInt(hexM[3], 16) / 255];
      }
      return null;
    }

    updateColorsFromTokens() {
      const cs = getComputedStyle(document.documentElement);
      const acc = cs.getPropertyValue("--accent").trim();
      const pRgb = this.parseRgbString(acc) || [0.48, 0.42, 1.0];
      const isDark = (document.documentElement.getAttribute("data-mode") || "dark") === "dark";

      this.colors.primary = pRgb;
      // Secondary: slightly rotated hue / lifted cyan-magenta harmony
      this.colors.secondary = [
        Math.min(1, pRgb[0] * 0.4 + 0.1),
        Math.min(1, pRgb[1] * 0.8 + 0.35),
        Math.min(1, pRgb[2] * 0.9 + 0.25),
      ];
      this.colors.deep = isDark
        ? [Math.max(0, pRgb[0] * 0.18), Math.max(0, pRgb[1] * 0.22), Math.max(0, pRgb[2] * 0.36 + 0.08)]
        : [0.85, 0.92, 0.98];
      this.colors.bg = isDark ? [0.02, 0.028, 0.048] : [0.91, 0.95, 0.98];
    }

    setColors(primaryHex, secondaryHex) {
      const p = this.parseRgbString(primaryHex);
      if (p) this.colors.primary = p;
      const s = this.parseRgbString(secondaryHex);
      if (s) this.colors.secondary = s;
    }

    setAudioData(bars) {
      if (!bars || bars.length === 0) {
        this.audioEnergy *= 0.92;
        this.audioBass *= 0.92;
        return;
      }
      let sum = 0;
      let bassSum = 0;
      const n = Math.min(32, bars.length);
      for (let i = 0; i < n; i++) {
        const v = bars[i] || 0;
        sum += v;
        if (i < 4) bassSum += v;
      }
      const rawEnergy = sum / n;
      const rawBass = bassSum / 4;
      this.audioEnergy = this.audioEnergy * 0.75 + rawEnergy * 0.25;
      this.audioBass = this.audioBass * 0.75 + rawBass * 0.25;
    }

    onVisibilityChange() {
      if (document.hidden) {
        this.stop();
      } else {
        const theme = document.documentElement.getAttribute("data-theme") || "analogue";
        if (theme === "digital") this.start();
      }
    }

    start() {
      if (this.running) return;
      this.running = true;
      this.rafId = requestAnimationFrame(this.boundTick);
    }

    stop() {
      this.running = false;
      if (this.rafId) {
        cancelAnimationFrame(this.rafId);
        this.rafId = 0;
      }
    }

    isRunning() {
      return this.running;
    }

    getCanvas() {
      return this.canvas;
    }

    tick(now) {
      if (!this.running) return;

      const theme = document.documentElement.getAttribute("data-theme") || "analogue";
      const fxLayer = document.querySelector(".fx-layer");
      const ambSetting = fxLayer ? fxLayer.dataset.ambient : "dither";

      if (theme !== "digital" || ambSetting === "off") {
        if (this.gl) this.gl.clear(this.gl.COLOR_BUFFER_BIT);
        this.stop();
        return;
      }

      // Sample audio spectrum from shared state if available
      if (window.S && window.S._barsRcv) {
        this.setAudioData(window.S._barsRcv);
      }

      // Smooth color interpolation (6% per frame)
      for (const k of ["primary", "secondary", "deep", "bg"]) {
        for (let i = 0; i < 3; i++) {
          this.curColors[k][i] += (this.colors[k][i] - this.curColors[k][i]) * 0.06;
        }
      }

      const gl = this.gl;
      gl.useProgram(this.program);

      const elapsed = (now - this.startTime) / 1000.0;
      const isDark = (document.documentElement.getAttribute("data-mode") || "dark") === "dark";

      gl.uniform1f(this.uniforms.u_time, elapsed);
      gl.uniform2f(this.uniforms.u_resolution, this.canvas.width, this.canvas.height);
      gl.uniform3fv(this.uniforms.u_color_primary, this.curColors.primary);
      gl.uniform3fv(this.uniforms.u_color_secondary, this.curColors.secondary);
      gl.uniform3fv(this.uniforms.u_color_deep, this.curColors.deep);
      gl.uniform3fv(this.uniforms.u_color_bg, this.curColors.bg);
      gl.uniform1f(this.uniforms.u_audio_energy, this.audioEnergy);
      gl.uniform1f(this.uniforms.u_audio_bass, this.audioBass);
      gl.uniform1f(this.uniforms.u_is_dark, isDark ? 1.0 : 0.0);
      gl.uniform1f(this.uniforms.u_intensity, 1.0);

      gl.drawArrays(gl.TRIANGLES, 0, 6);

      // If reduced motion is requested, render one static frame and pause
      const root = document.documentElement;
      if (root.getAttribute("data-reduced-motion") === "true" || root.getAttribute("data-motion") === "reduced" || root.getAttribute("data-motion") === "off") {
        this.stop();
        return;
      }

      this.rafId = requestAnimationFrame(this.boundTick);
    }
  }

  const ambientInstance = new HalftoneAmbientShader();

  function bootAmbient() {
    ambientInstance.init();
    if (typeof MutationObserver === "undefined") return;
    // React to theme switches
    const observer = new MutationObserver(() => {
      const theme = document.documentElement.getAttribute("data-theme") || "analogue";
      if (theme === "digital") {
        if (!ambientInstance.isRunning()) ambientInstance.start();
      } else {
        if (ambientInstance.isRunning()) ambientInstance.stop();
      }
    });
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "data-mode", "data-motion"] });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", bootAmbient, { once: true });
  } else {
    bootAmbient();
  }

  window.HalftoneAmbient = ambientInstance;
})();

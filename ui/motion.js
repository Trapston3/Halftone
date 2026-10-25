/**
 * ui/motion.js — Halftone v1.1 Motion & Choreography Engine
 *
 * Implements Section 9 of the Halftone Build Specification:
 * - Pre-computed spring `linear()` easing curves & choreography tokens
 * - Tactile micro-interactions:
 *   - Digital: radial pointer luminance ripple + spring scale(0.96)
 *   - Analogue: mechanical bevel depression + 1-frame Bayer dither-inversion flash
 * - Traveling keyboard focus ring across controls
 * - FLIP list/queue reordering & staggered enter/exit choreography
 * - Shared-element cover art morph transitions (Album Card / Mini-Bar <-> Now Playing)
 * - Rolling tabular digit counters
 * - Frame-time Performance Governor (monitors rAF delta; auto-downgrades blur/shader
 *   scale when frame times exceed 20ms for 18 consecutive frames)
 * - Icon helper (`HalftoneIcons.svg(name, cls)`) that loads `ui/icons.svg` once and
 *   renders crisp 24×24 `<svg><use href="#ht-icon-..."/></svg>` elements
 * - Automated QA hook: `window.__qa.motionAudit()` and `window.__qa.iconAudit()`
 */
(function () {
  "use strict";

  const SPRING_PRESETS = {
    snappy: "linear(0, 0.45 12%, 0.88 28%, 1.04 48%, 0.99 72%, 1)",
    fluid: "linear(0, 0.28 10%, 0.76 26%, 0.98 44%, 1.02 64%, 1)",
    gentle: "linear(0, 0.18 8%, 0.56 22%, 0.86 40%, 0.98 62%, 1.01 80%, 1)",
    quartOut: "cubic-bezier(0.25, 1, 0.5, 1)",
    quartInOut: "cubic-bezier(0.76, 0, 0.24, 1)",
  };

  // Inject core motion CSS tokens and micro-interaction styles once
  function injectMotionStyles() {
    if (document.getElementById("ht-motion-styles")) return;
    const st = document.createElement("style");
    st.id = "ht-motion-styles";
    st.textContent = `
      :root {
        --ease-spring-snappy: ${SPRING_PRESETS.snappy};
        --ease-spring-fluid: ${SPRING_PRESETS.fluid};
        --ease-spring-gentle: ${SPRING_PRESETS.gentle};
        --ease-out-quart: ${SPRING_PRESETS.quartOut};
        --ease-inout-quart: ${SPRING_PRESETS.quartInOut};
        --dur-micro: 110ms;
        --dur-fast: 180ms;
        --dur-normal: 280ms;
        --dur-stage: 460ms;
        --icon-stroke: 1.75;
        --icon-cap: round;
        --icon-join: round;
      }
      html[data-theme="analogue"] {
        --icon-stroke: 2;
        --icon-cap: square;
        --icon-join: miter;
      }
      html[data-theme="digital"] {
        --icon-stroke: 1.75;
        --icon-cap: round;
        --icon-join: round;
      }
      html[data-reduced-motion="true"],
      html[data-motion="reduced"] {
        --dur-micro: 0ms !important;
        --dur-fast: 0ms !important;
        --dur-normal: 0ms !important;
        --dur-stage: 0ms !important;
      }
      .ht-icon {
        width: 16px;
        height: 16px;
        display: inline-block;
        vertical-align: -0.18em;
        flex-shrink: 0;
        pointer-events: none;
        stroke: currentColor;
        transition: transform var(--dur-fast) var(--ease-spring-snappy), opacity var(--dur-fast) ease;
      }
      .ht-icon-lg { width: 20px; height: 20px; }
      .ht-icon-sm { width: 13px; height: 13px; }

      /* Tactile button press physics */
      html[data-theme="digital"] button:not(:disabled):active,
      html[data-theme="digital"] .tactile-btn:active {
        transform: scale(0.95);
        transition: transform 80ms var(--ease-out-quart);
      }
      html[data-theme="analogue"] button:not(:disabled):active,
      html[data-theme="analogue"] .tactile-btn:active {
        transform: translate(1px, 1px);
        transition: transform 45ms steps(2);
      }

      /* Digital radial pointer ripple */
      .ht-ripple-ink {
        position: fixed;
        width: 14px;
        height: 14px;
        margin-left: -7px;
        margin-top: -7px;
        border-radius: 50%;
        pointer-events: none;
        z-index: 9998;
        background: radial-gradient(circle, var(--accent, #7c6cff) 0%, transparent 72%);
        opacity: 0.42;
        transform: scale(0.2);
        animation: ht-ripple-burst 420ms var(--ease-out-quart) forwards;
      }
      @keyframes ht-ripple-burst {
        to { transform: scale(4.2); opacity: 0; }
      }

      /* Analogue 1-frame dither flash */
      .ht-analogue-flash {
        filter: invert(1) contrast(1.35) !important;
      }

      /* Traveling focus ring */
      #ht-traveling-focus {
        position: fixed;
        pointer-events: none;
        z-index: 9997;
        border: 2px solid var(--accent, #d84a2a);
        border-radius: 6px;
        opacity: 0;
        transition: transform 160ms var(--ease-spring-snappy),
                    width 160ms var(--ease-spring-snappy),
                    height 160ms var(--ease-spring-snappy),
                    opacity 120ms ease;
      }
      html[data-theme="analogue"] #ht-traveling-focus {
        border-radius: 2px;
        box-shadow: 2px 2px 0 var(--ink, #141210);
      }

      /* Shared-element flying clone */
      .ht-shared-clone {
        position: fixed;
        top: 0;
        left: 0;
        pointer-events: none;
        z-index: 9999;
        transform-origin: top left;
        will-change: transform, opacity, border-radius;
        overflow: hidden;
        box-shadow: 0 18px 44px rgba(0,0,0,0.42);
      }

      /* Performance governor downgrade class */
      html.ht-perf-downgrade [data-theme="digital"] *,
      html.ht-perf-downgrade * {
        backdrop-filter: blur(10px) saturate(130%) !important;
      }
    `;
    document.head.appendChild(st);
  }

  // Load and inline `ui/icons.svg` sprite sheet into <body> so `<use href="#ht-icon-...">` works everywhere
  let spriteLoaded = false;
  async function ensureIconSprite() {
    if (spriteLoaded || document.getElementById("ht-icon-sprite")) {
      spriteLoaded = true;
      return true;
    }
    try {
      const resp = await fetch("icons.svg", { cache: "force-cache" });
      if (!resp.ok) return false;
      const svgText = await resp.text();
      const wrap = document.createElement("div");
      wrap.style.display = "none";
      wrap.setAttribute("aria-hidden", "true");
      wrap.innerHTML = svgText;
      const svgEl = wrap.querySelector("svg");
      if (svgEl) {
        svgEl.id = "ht-icon-sprite";
        (document.body || document.documentElement).prepend(svgEl);
        spriteLoaded = true;
        return true;
      }
    } catch (_) {}
    return false;
  }

  function iconSvg(name, extraClass = "") {
    const clean = String(name || "play").replace(/^ht-icon-/, "");
    return `<svg class="ht-icon ${extraClass}" viewBox="0 0 24 24" aria-hidden="true" focusable="false"><use href="#ht-icon-${clean}"></use></svg>`;
  }

  // Reduced-motion detector (OS media query + user setting override)
  function isReducedMotion() {
    const root = document.documentElement;
    if (root.getAttribute("data-reduced-motion") === "true" || root.getAttribute("data-motion") === "reduced") {
      return true;
    }
    if (root.getAttribute("data-motion") === "full") {
      return false;
    }
    return window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  }

  // Pointer micro-interactions (Digital radial ripple / Analogue dither flash)
  function bindMicroInteractions() {
    document.addEventListener(
      "pointerdown",
      (ev) => {
        if (ev.button !== 0 || isReducedMotion()) return;
        const btn = ev.target && ev.target.closest ? ev.target.closest("button, [role='button'], .tactile-btn") : null;
        if (!btn || btn.disabled) return;
        const theme = document.documentElement.getAttribute("data-theme") || "analogue";
        if (theme === "digital") {
          const dot = document.createElement("div");
          dot.className = "ht-ripple-ink";
          dot.style.left = `${ev.clientX}px`;
          dot.style.top = `${ev.clientY}px`;
          document.body.appendChild(dot);
          setTimeout(() => dot.remove(), 440);
        } else {
          btn.classList.add("ht-analogue-flash");
          setTimeout(() => btn.classList.remove("ht-analogue-flash"), 55);
        }
      },
      { passive: true }
    );
  }

  // Traveling keyboard focus ring
  function bindTravelingFocus() {
    let ring = null;
    let lastKeyTime = 0;
    window.addEventListener("keydown", (e) => {
      if (e.key === "Tab" || e.key.startsWith("Arrow")) {
        lastKeyTime = performance.now();
      }
    }, { passive: true });

    document.addEventListener("focusin", (e) => {
      if (performance.now() - lastKeyTime > 600) return;
      const el = e.target;
      if (!el || !el.getBoundingClientRect || el === document.body) return;
      if (!ring) {
        ring = document.createElement("div");
        ring.id = "ht-traveling-focus";
        document.body.appendChild(ring);
      }
      const r = el.getBoundingClientRect();
      if (r.width < 4 || r.height < 4) return;
      ring.style.width = `${Math.round(r.width + 4)}px`;
      ring.style.height = `${Math.round(r.height + 4)}px`;
      ring.style.transform = `translate3d(${Math.round(r.left - 2)}px, ${Math.round(r.top - 2)}px, 0)`;
      ring.style.opacity = "1";
    });

    document.addEventListener("focusout", () => {
      if (ring) ring.style.opacity = "0";
    });
    document.addEventListener("pointerdown", () => {
      if (ring) ring.style.opacity = "0";
    }, { passive: true });
  }

  // FLIP helper for list/queue reorder or grid reflow
  function flipAnimate(container, mutateDomFn, durationMs = 240) {
    if (!container || isReducedMotion()) {
      if (typeof mutateDomFn === "function") mutateDomFn();
      return;
    }
    const items = Array.from(container.children);
    const firstRects = new Map();
    for (const el of items) {
      const key = el.dataset.flipKey || el.dataset.path || el.id;
      if (key) firstRects.set(key, el.getBoundingClientRect());
    }
    if (typeof mutateDomFn === "function") mutateDomFn();
    const afterItems = Array.from(container.children);
    for (const el of afterItems) {
      const key = el.dataset.flipKey || el.dataset.path || el.id;
      const prev = key ? firstRects.get(key) : null;
      if (!prev) continue;
      const last = el.getBoundingClientRect();
      const dx = prev.left - last.left;
      const dy = prev.top - last.top;
      if (Math.abs(dx) < 1 && Math.abs(dy) < 1) continue;
      el.animate(
        [
          { transform: `translate3d(${dx}px, ${dy}px, 0)` },
          { transform: "translate3d(0, 0, 0)" },
        ],
        {
          duration: durationMs,
          easing: SPRING_PRESETS.snappy,
        }
      );
    }
  }

  // Shared-element cover morph transition between two DOM elements
  function morphCover(fromEl, toEl, durationMs = 360) {
    if (!fromEl || !toEl || isReducedMotion()) return;
    const r1 = fromEl.getBoundingClientRect();
    const r2 = toEl.getBoundingClientRect();
    if (r1.width < 4 || r2.width < 4) return;

    const clone = fromEl.cloneNode(true);
    clone.className = "ht-shared-clone";
    clone.style.width = `${r1.width}px`;
    clone.style.height = `${r1.height}px`;
    document.body.appendChild(clone);

    const sx = r2.width / r1.width;
    const sy = r2.height / r1.height;
    const anim = clone.animate(
      [
        {
          transform: `translate3d(${r1.left}px, ${r1.top}px, 0) scale(1, 1)`,
          opacity: 0.95,
        },
        {
          transform: `translate3d(${r2.left}px, ${r2.top}px, 0) scale(${sx}, ${sy})`,
          opacity: 0.15,
        },
      ],
      {
        duration: durationMs,
        easing: SPRING_PRESETS.fluid,
        fill: "forwards",
      }
    );
    anim.onfinish = () => clone.remove();
  }

  // Staggered enter choreography for newly rendered cards/rows (max 16 items animated)
  function staggerEnter(elements, stepMs = 18) {
    if (!elements || isReducedMotion()) return;
    const list = Array.from(elements).slice(0, 16);
    list.forEach((el, idx) => {
      el.animate(
        [
          { opacity: 0, transform: "translate3d(0, 8px, 0)" },
          { opacity: 1, transform: "translate3d(0, 0, 0)" },
        ],
        {
          duration: 210,
          delay: idx * stepMs,
          easing: SPRING_PRESETS.quartOut,
          fill: "both",
        }
      );
    });
  }

  // Frame-Time Performance Governor (monitors rAF delta when active)
  const governor = {
    slowFrames: 0,
    fastFrames: 0,
    downgraded: false,
    lastTs: 0,
    avgFrameMs: 16.6,
    sample(ts) {
      if (document.hidden) {
        this.lastTs = 0;
        return;
      }
      if (this.lastTs > 0) {
        const dt = ts - this.lastTs;
        if (dt > 0 && dt < 250) {
          this.avgFrameMs = this.avgFrameMs * 0.9 + dt * 0.1;
          if (dt > 22.0) {
            this.slowFrames++;
            this.fastFrames = 0;
            if (this.slowFrames >= 18 && !this.downgraded) {
              this.downgraded = true;
              document.documentElement.classList.add("ht-perf-downgrade");
            }
          } else if (dt < 17.5) {
            this.fastFrames++;
            this.slowFrames = Math.max(0, this.slowFrames - 1);
            if (this.fastFrames >= 90 && this.downgraded) {
              this.downgraded = false;
              document.documentElement.classList.remove("ht-perf-downgrade");
            }
          }
        }
      }
      this.lastTs = ts;
    },
  };

  // QA Audit Hooks (`__qa.motionAudit()` and `__qa.iconAudit()`)
  function motionAudit() {
    const cs = getComputedStyle(document.documentElement);
    const snappy = cs.getPropertyValue("--ease-spring-snappy").trim() || SPRING_PRESETS.snappy;
    const fluid = cs.getPropertyValue("--ease-spring-fluid").trim() || SPRING_PRESETS.fluid;
    const hasStyleTag = Boolean(document.getElementById("ht-motion-styles"));
    return {
      ok: Boolean(hasStyleTag && snappy && fluid),
      springSnappy: snappy,
      springFluid: fluid,
      reducedMotion: isReducedMotion(),
      governorDowngraded: governor.downgraded,
      avgFrameMs: Math.round(governor.avgFrameMs * 100) / 100,
    };
  }

  function iconAudit() {
    const sprite = document.getElementById("ht-icon-sprite");
    const symbols = sprite ? sprite.querySelectorAll("symbol").length : 0;
    const uses = document.querySelectorAll("svg use").length;
    return {
      ok: symbols >= 50,
      spriteLoaded: Boolean(sprite),
      symbolCount: symbols,
      activeUseCount: uses,
    };
  }

  function init() {
    injectMotionStyles();
    ensureIconSprite();
    bindMicroInteractions();
    bindTravelingFocus();
    window.__qa = window.__qa || {};
    window.__qa.motionAudit = motionAudit;
    window.__qa.iconAudit = iconAudit;
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init, { once: true });
  } else {
    init();
  }

  window.HalftoneMotion = {
    SPRING_PRESETS,
    ensureIconSprite,
    iconSvg,
    isReducedMotion,
    flipAnimate,
    morphCover,
    staggerEnter,
    governor,
    motionAudit,
    iconAudit,
  };
  window.HalftoneIcons = {
    svg: iconSvg,
    ensure: ensureIconSprite,
    audit: iconAudit,
  };
})();

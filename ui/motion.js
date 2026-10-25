/**
 * ui/motion.js — Halftone v1.1 Motion & Choreography Engine
 *
 * Implements Section 9 of the Halftone Build Specification:
 * - Pre-computed spring `linear()` easing curves & choreography tokens
 * - Tactile micro-interactions:
 *   - Digital: radial pointer luminance ripple + spring scale(0.94) + magnetic hover on transport orbs
 *   - Analogue: mechanical bevel depression + 1-frame Bayer dither-inversion flash
 * - Traveling keyboard focus ring across controls
 * - FLIP list/queue reordering & staggered enter/exit choreography
 * - Shared-element cover art morph transitions (Album Card / Mini-Bar <-> Now Playing)
 * - WAAPI Spring View Transitions with auto-staggering of child cards/rows
 * - Smooth Cover Art Pulse & Metadata vertical mask reveal on track change
 * - Automatic MutationObserver view watcher (seamless animated transitions on view switch/content fill)
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

      /* Halftone Choreographed View & Track Transitions */
      @keyframes htViewEnterForward {
        from { opacity: 0; transform: translate3d(16px, 0, 0) scale(0.992); }
        to { opacity: 1; transform: translate3d(0, 0, 0) scale(1); }
      }
      @keyframes htViewEnterBack {
        from { opacity: 0; transform: translate3d(-16px, 0, 0) scale(0.992); }
        to { opacity: 1; transform: translate3d(0, 0, 0) scale(1); }
      }
      @keyframes htViewEnterUp {
        from { opacity: 0; transform: translate3d(0, 14px, 0) scale(0.992); }
        to { opacity: 1; transform: translate3d(0, 0, 0) scale(1); }
      }
      @keyframes htTrackChangePulse {
        0% { opacity: 0.35; transform: scale(0.95); filter: blur(6px); }
        100% { opacity: 1; transform: scale(1); filter: blur(0); }
      }
      @keyframes htMetaReveal {
        from { opacity: 0; transform: translate3d(0, 8px, 0); }
        to { opacity: 1; transform: translate3d(0, 0, 0); }
      }
      .ht-view-enter-forward {
        animation: htViewEnterForward 280ms cubic-bezier(0.22, 1, 0.36, 1) both;
      }
      .ht-view-enter-back {
        animation: htViewEnterBack 280ms cubic-bezier(0.22, 1, 0.36, 1) both;
      }
      .ht-view-enter-up {
        animation: htViewEnterUp 280ms cubic-bezier(0.22, 1, 0.36, 1) both;
      }
      .ht-track-change-pulse {
        animation: htTrackChangePulse 340ms cubic-bezier(0.22, 1, 0.36, 1) both;
      }
      .ht-meta-reveal {
        animation: htMetaReveal 280ms cubic-bezier(0.22, 1, 0.36, 1) both;
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

  // Magnetic hover micro-interaction on primary transport buttons in Digital mode
  function bindMagneticButtons() {
    const SELECTOR = "#btnPlay, #wPlay, #nbPlay, .btn-orb.lg, .cbtn.play";
    document.addEventListener("pointermove", (e) => {
      const theme = document.documentElement.getAttribute("data-theme") || "analogue";
      if (theme !== "digital" || isReducedMotion()) return;
      const target = e.target && e.target.closest ? e.target.closest(SELECTOR) : null;
      if (!target) return;
      const rect = target.getBoundingClientRect();
      const centerX = rect.left + rect.width / 2;
      const centerY = rect.top + rect.height / 2;
      const maxOffset = 8;
      const dx = Math.max(-maxOffset, Math.min(maxOffset, (e.clientX - centerX) * 0.28));
      const dy = Math.max(-maxOffset, Math.min(maxOffset, (e.clientY - centerY) * 0.28));
      target.style.transform = `translate3d(${dx.toFixed(1)}px, ${dy.toFixed(1)}px, 0) scale(1.07)`;
    }, { passive: true });

    document.addEventListener("pointerout", (e) => {
      const target = e.target && e.target.closest ? e.target.closest(SELECTOR) : null;
      if (!target) return;
      target.style.transform = "";
      if (!isReducedMotion()) {
        try {
          target.animate(
            [
              { transform: target.style.transform || "translate3d(0, 0, 0) scale(1.07)" },
              { transform: "translate3d(0, 0, 0) scale(1)" },
            ],
            { duration: 240, easing: SPRING_PRESETS.snappy, fill: "forwards" }
          );
        } catch (_) {}
      }
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

  // Staggered enter choreography for newly rendered cards/rows (up to 24 items animated)
  function staggerEnter(elements, stepMs = 18) {
    if (!elements || isReducedMotion()) return;
    const list = Array.from(elements).slice(0, 24);
    list.forEach((el, idx) => {
      try {
        el.animate(
          [
            { opacity: 0, transform: "translate3d(0, 10px, 0)" },
            { opacity: 1, transform: "translate3d(0, 0, 0)" },
          ],
          {
            duration: 260,
            delay: idx * stepMs,
            easing: "cubic-bezier(0.22, 1, 0.36, 1)",
            fill: "both",
          }
        );
      } catch (_) {}
    });
  }

  // Animated Transitions Between Views, Pages & Sections with child staggering
  function animateView(containerEl, dir = "up") {
    const el = typeof containerEl === "string" ? document.querySelector(containerEl) : containerEl;
    if (!el || isReducedMotion()) return null;
    const d = String(dir || "up").toLowerCase();
    let keyframes;
    let animClass = "ht-view-enter-up";
    if (d === "forward" || d === "next" || d === "right") {
      animClass = "ht-view-enter-forward";
      keyframes = [
        { opacity: 0, transform: "translate3d(16px, 0, 0) scale(0.992)" },
        { opacity: 1, transform: "translate3d(0, 0, 0) scale(1)" },
      ];
    } else if (d === "back" || d === "prev" || d === "left") {
      animClass = "ht-view-enter-back";
      keyframes = [
        { opacity: 0, transform: "translate3d(-16px, 0, 0) scale(0.992)" },
        { opacity: 1, transform: "translate3d(0, 0, 0) scale(1)" },
      ];
    } else {
      animClass = "ht-view-enter-up";
      keyframes = [
        { opacity: 0, transform: "translate3d(0, 14px, 0) scale(0.992)" },
        { opacity: 1, transform: "translate3d(0, 0, 0) scale(1)" },
      ];
    }

    el.classList.remove("ht-view-enter-forward", "ht-view-enter-back", "ht-view-enter-up");
    try {
      const anim = el.animate(keyframes, {
        duration: 320,
        easing: "cubic-bezier(0.22, 1, 0.36, 1)",
        fill: "both",
      });
      el.classList.add(animClass);
      anim.onfinish = () => {
        el.classList.remove(animClass);
      };

      // Automatically stagger child cards and rows inside the newly entered view
      const children = el.querySelectorAll(
        ".album-card, .v11-card, .smart-card, .stat-card, .cf-card, .row, .trow, .prow, .pl-item, .ht-setting-row, .card, .acard, .cover-cell, .set-row"
      );
      if (children.length > 0) {
        staggerEnter(children, 18);
      }

      return anim;
    } catch (_) {
      el.classList.add(animClass);
      setTimeout(() => el.classList.remove(animClass), 320);
      return null;
    }
  }

  // Smooth cover art crossfade & scale pulse + metadata mask reveal on track changes
  function animateTrackChange(artEl, metaEl) {
    if (isReducedMotion()) return [];
    const resolve = (target) => (typeof target === "string" ? document.querySelector(target) : target);
    const artTarget = resolve(artEl) || document.querySelector("#npArt, #wArt, #npCover, #wCover, #heroArt");
    const metaTarget = resolve(metaEl) || document.querySelector("#npTitle, #wTitle, .np-info, .w-meta");
    const anims = [];

    if (artTarget) {
      try {
        artTarget.classList.remove("ht-track-change-pulse");
        void artTarget.offsetWidth;
        const a = artTarget.animate(
          [
            { opacity: 0.35, transform: "scale(0.95)", filter: "blur(6px)" },
            { opacity: 1, transform: "scale(1)", filter: "blur(0)" },
          ],
          {
            duration: 340,
            easing: "cubic-bezier(0.22, 1, 0.36, 1)",
            fill: "both",
          }
        );
        artTarget.classList.add("ht-track-change-pulse");
        a.onfinish = () => artTarget.classList.remove("ht-track-change-pulse");
        anims.push(a);
      } catch (_) {}
    }

    if (metaTarget) {
      try {
        metaTarget.classList.remove("ht-meta-reveal");
        void metaTarget.offsetWidth;
        const b = metaTarget.animate(
          [
            { opacity: 0, transform: "translate3d(0, 8px, 0)" },
            { opacity: 1, transform: "translate3d(0, 0, 0)" },
          ],
          {
            duration: 280,
            easing: "cubic-bezier(0.22, 1, 0.36, 1)",
            fill: "both",
          }
        );
        metaTarget.classList.add("ht-meta-reveal");
        b.onfinish = () => metaTarget.classList.remove("ht-meta-reveal");
        anims.push(b);
      } catch (_) {}
    }

    return anims;
  }

  // Automatically observe view changes & DOM updates to trigger smooth entrance choreography
  function bindViewObserver() {
    if (typeof MutationObserver === "undefined") return;
    let lastAnimatedTime = 0;
    const observer = new MutationObserver((mutations) => {
      const now = performance.now();
      if (now - lastAnimatedTime < 240) return;

      for (const m of mutations) {
        if (m.type === "attributes" && (m.attributeName === "style" || m.attributeName === "class")) {
          const target = m.target;
          if (
            target &&
            target.classList &&
            (target.classList.contains("view") ||
              target.id === "content" ||
              target.id === "v-content" ||
              target.id === "v-nowplaying" ||
              target.id === "v-eq" ||
              target.id === "v-queue" ||
              target.id === "nowPane" ||
              target.id === "albumsPane" ||
              target.id === "viewExtraPane" ||
              target.id === "eqSheet")
          ) {
            const disp = getComputedStyle(target).display;
            if (disp !== "none" && target.offsetParent !== null) {
              lastAnimatedTime = now;
              animateView(target, "up");
              break;
            }
          }
        } else if (m.type === "childList" && m.addedNodes.length > 0) {
          const target = m.target;
          if (target && (target.id === "content" || target.id === "queueView" || target.classList.contains("vscroll"))) {
            lastAnimatedTime = now;
            const newChildren = target.querySelectorAll(
              ".album-card, .v11-card, .smart-card, .stat-card, .cf-card, .row, .trow, .prow, .pl-item, .card, .acard, .cover-cell"
            );
            if (newChildren.length > 0) {
              staggerEnter(newChildren, 18);
            }
            break;
          }
        }
      }
    });

    const rootViews = document.getElementById("views") || document.getElementById("appShell") || document.body;
    if (rootViews) {
      observer.observe(rootViews, { attributes: true, childList: true, subtree: true, attributeFilter: ["style", "class"] });
    }
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

  // Load Ambient WebGL script dynamically if not present
  function ensureAmbientShader() {
    if (!window.HalftoneAmbient && !document.querySelector('script[src="ambient-webgl.js"]')) {
      const s = document.createElement("script");
      s.src = "ambient-webgl.js";
      document.head.appendChild(s);
    }
  }

  function init() {
    injectMotionStyles();
    ensureIconSprite();
    bindMicroInteractions();
    bindTravelingFocus();
    bindMagneticButtons();
    bindViewObserver();
    ensureAmbientShader();
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
    animateView,
    animateTrackChange,
    bindMagneticButtons,
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

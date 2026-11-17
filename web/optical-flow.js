// Fast exhaustive integer SAD optical flow with parabolic sub-pixel refinement
// modeled after dedicated optical mouse sensors (48x36 pixel array, 100% step-1 search).

export const FLOW_WIDTH = 48;
export const FLOW_HEIGHT = 36;

export function createOpticalState() {
  return {
    prevGray: null,
    smoothDx: 0,
    smoothDy: 0,
  };
}

export function resetOpticalState(state, clearFrame = true) {
  state.smoothDx = 0;
  state.smoothDy = 0;
  if (clearFrame) {
    state.prevGray = null;
  }
}

export function rgbaToGrayscale(rgba, width = FLOW_WIDTH, height = FLOW_HEIGHT, out = null) {
  const len = width * height;
  const gray = out && out.length === len ? out : new Uint8Array(len);
  for (let i = 0, p = 0; i < len; i++, p += 4) {
    gray[i] = (rgba[p] * 77 + rgba[p + 1] * 150 + rgba[p + 2] * 29) >> 8;
  }
  return gray;
}

function clamp(v, lo, hi) {
  return v < lo ? lo : v > hi ? hi : v;
}

export function estimateOpticalShift(
  prevGray,
  currGray,
  width = FLOW_WIDTH,
  height = FLOW_HEIGHT,
  opts = {},
) {
  const maxShiftX = opts.maxShiftX ?? 10;
  const maxShiftY = opts.maxShiftY ?? 8;
  const minContrast = opts.minContrast ?? 3.2;

  if (!prevGray || !currGray || prevGray.length < width * height || currGray.length < width * height) {
    return { shiftX: 0, shiftY: 0, confidence: 0, contrast: 0, reason: 'no-frame' };
  }

  const x0 = maxShiftX + 1;
  const x1 = width - maxShiftX - 1;
  const y0 = maxShiftY + 1;
  const y1 = height - maxShiftY - 1;
  if (x1 <= x0 + 4 || y1 <= y0 + 4) {
    return { shiftX: 0, shiftY: 0, confidence: 0, contrast: 0, reason: 'bad-size' };
  }

  // 1. Check center template contrast (mean absolute deviation)
  let sum = 0;
  let tplCount = 0;
  for (let y = y0; y < y1; y++) {
    const row = y * width;
    for (let x = x0; x < x1; x++) {
      sum += prevGray[row + x];
      tplCount++;
    }
  }
  const mean = sum / tplCount;
  let absDev = 0;
  for (let y = y0; y < y1; y++) {
    const row = y * width;
    for (let x = x0; x < x1; x++) {
      absDev += Math.abs(prevGray[row + x] - mean);
    }
  }
  const contrast = absDev / tplCount;
  if (contrast < minContrast) {
    return { shiftX: 0, shiftY: 0, confidence: 0, contrast, reason: 'low-texture' };
  }

  // 2. Exhaustive integer SAD search across full search window (zero aliasing)
  const gridW = maxShiftX * 2 + 1;
  const gridH = maxShiftY * 2 + 1;
  const sadGrid = new Float64Array(gridW * gridH);

  let bestX = 0;
  let bestY = 0;
  let bestSad = Infinity;
  let sumSad = 0;

  for (let sy = -maxShiftY; sy <= maxShiftY; sy++) {
    const gy = (sy + maxShiftY) * gridW;
    for (let sx = -maxShiftX; sx <= maxShiftX; sx++) {
      let sad = 0;
      for (let y = y0; y < y1; y++) {
        const prevRow = y * width;
        const currRow = (y + sy) * width + sx;
        for (let x = x0; x < x1; x++) {
          sad += Math.abs(currGray[currRow + x] - prevGray[prevRow + x]);
        }
      }
      sadGrid[gy + (sx + maxShiftX)] = sad;
      sumSad += sad;
      if (sad < bestSad) {
        bestSad = sad;
        bestX = sx;
        bestY = sy;
      }
    }
  }

  // 3. Confidence check: compare best match sharpness vs average landscape SAD
  const meanSad = sumSad / sadGrid.length;
  const ratio = meanSad > 0 ? bestSad / meanSad : 1;
  const confidence = clamp(1 - ratio, 0, 1);

  if (confidence < 0.16) {
    return { shiftX: 0, shiftY: 0, confidence, contrast, reason: 'uncertain' };
  }

  // 4. Parabolic sub-pixel interpolation (reusing computed SAD grid)
  const gx = bestX + maxShiftX;
  const gy = bestY + maxShiftY;

  let subX = 0;
  if (Math.abs(bestX) < maxShiftX) {
    const sadL = sadGrid[gy * gridW + (gx - 1)];
    const sadR = sadGrid[gy * gridW + (gx + 1)];
    const denomX = sadL - 2 * bestSad + sadR;
    if (denomX > 0) {
      subX = clamp((sadL - sadR) / (2 * denomX), -0.5, 0.5);
    }
  }

  let subY = 0;
  if (Math.abs(bestY) < maxShiftY) {
    const sadU = sadGrid[(gy - 1) * gridW + gx];
    const sadD = sadGrid[(gy + 1) * gridW + gx];
    const denomY = sadU - 2 * bestSad + sadD;
    if (denomY > 0) {
      subY = clamp((sadU - sadD) / (2 * denomY), -0.5, 0.5);
    }
  }

  return {
    shiftX: bestX + subX,
    shiftY: bestY + subY,
    confidence,
    contrast,
    reason: 'ok',
  };
}

export function opticalShiftToCursorDelta(flowResult, state, opts = {}) {
  const {
    sensitivity = 1.4,
    invertX = false,
    invertY = false,
  } = opts;

  if (!flowResult || flowResult.reason !== 'ok') {
    state.smoothDx *= 0.4;
    state.smoothDy *= 0.4;
    return { dx: 0, dy: 0 };
  }

  // Camera moving +right / +forward causes stationary background features to shift -X / +Y
  const rawDx = -flowResult.shiftX;
  const rawDy = -flowResult.shiftY;

  const smoothAlpha = 0.65;
  state.smoothDx = state.smoothDx * (1 - smoothAlpha) + rawDx * smoothAlpha;
  state.smoothDy = state.smoothDy * (1 - smoothAlpha) + rawDy * smoothAlpha;

  const mag = Math.hypot(state.smoothDx, state.smoothDy);
  const deadzone = 0.14;
  if (mag <= deadzone) {
    return { dx: 0, dy: 0 };
  }

  const effMag = mag - deadzone;
  const accel = 0.85 + Math.min(1.5, effMag / 5.5);
  const scale = (effMag * 3.8 * sensitivity * accel) / mag;

  let dx = state.smoothDx * scale;
  let dy = state.smoothDy * scale;

  if (invertX) dx = -dx;
  if (invertY) dy = -dy;

  return { dx, dy };
}

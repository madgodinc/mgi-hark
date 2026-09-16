// American Sign Language fingerspelling, 26 letters. Checked against the
// public-domain ASL letter drawings on Wikimedia Commons ("Sign language A.svg"
// and the rest of that set, by wpclipart.com). J and Z are movements.
//
// Angles are explained in ../hand.js. pos moves the whole hand, in metres.

const S = [0, 0, 0, 0];
const F = [0, 88, 98, 62];
const BENT90 = [0, 86, 0, 0];

const fist = { index: F, middle: F, ring: F, pinky: F, thumb: [42, 12, 22, 18], touch: "middle-finger-phalanx-intermediate" };
const overRing = { touch: "ring-finger-phalanx-intermediate" };
const curved = [0, 35, 50, 25];

const still = (pose) => [pose];
const move = (pose, ...deltas) => [pose, ...deltas.map((d) => ({ ...pose, ...d }))];

const I = { ...fist, pinky: S, touch: "index-finger-phalanx-intermediate" };
const K = { ...fist, index: [4, 0, 0, 0], middle: [-4, 45, 0, 0], thumb: [30, 20, 0, 0], touch: "middle-finger-phalanx-proximal" };
const G = { ...fist, index: S, thumb: [10, 20, 0, 0], touch: undefined, rot: [0, -70, 90] };

export const EN = {
  A: still({ ...fist, thumb: [0, 0, 0, 0], touch: "index-finger-phalanx-intermediate", rot: [0, 30, 0] }),
  B: still({ thumb: [60, 10, 30, 20], touch: "ring-finger-phalanx-proximal" }),
  C: still({ rot: [0, 60, 0], index: curved, middle: curved, ring: curved, pinky: curved, thumb: [10, 40, 15, 5] }),
  D: still({ index: S, middle: [0, 50, 60, 40], ring: [0, 52, 62, 40], pinky: [0, 55, 62, 40], thumb: [50, 30, 20, 10], touch: "middle-finger-tip" }),
  E: still({ index: [0, 70, 100, 60], middle: [0, 72, 100, 60], ring: [0, 74, 100, 60], pinky: [0, 76, 100, 60], thumb: [75, 5, 30, 20], touch: "ring-finger-phalanx-intermediate" }),
  F: still({ index: [0, 45, 60, 40], thumb: [50, 30, 20, 10], touch: "index-finger-tip", ring: [-4, 0, 0, 0], pinky: [-10, 0, 0, 0] }),
  G: still(G),
  H: still({ ...fist, index: S, middle: S, rot: [0, 40, 90] }),
  I: still(I),
  J: move(I, { rot: [0, 0, -40], pos: [0.01, -0.03, 0] }, { rot: [0, 40, -70], pos: [-0.02, -0.05, 0] }),
  K: still(K),
  L: still({ ...fist, index: S, thumb: [0, 60, 0, 0], touch: undefined }),
  M: still({ ...fist, index: [0, 80, 70, 40], middle: [0, 80, 70, 40], ring: [0, 80, 70, 40], thumb: [70, 10, 20, 10], touch: "pinky-finger-phalanx-proximal" }),
  N: still({ ...fist, index: [0, 80, 70, 40], middle: [0, 80, 70, 40], thumb: [60, 10, 20, 10], touch: "ring-finger-phalanx-proximal" }),
  O: still({ rot: [0, 50, 0], index: [0, 40, 60, 35], middle: [0, 40, 62, 35], ring: [0, 42, 62, 35], pinky: [0, 44, 62, 35], thumb: [40, 30, 20, 10], touch: "index-finger-tip" }),
  P: still({ ...K, rot: [0, 40, 150] }),
  Q: still({ ...G, rot: [0, 40, 170] }),
  R: still({ ...fist, ...overRing, index: S, middle: [20, 0, 0, 0], rot: [0, 30, 0] }),
  S: still({ ...fist, thumb: [60, 10, 30, 20] }),
  T: still({ ...fist, index: [0, 75, 90, 60], thumb: [45, 20, 10, 0], touch: "index-finger-phalanx-proximal" }),
  U: still({ ...fist, ...overRing, index: S, middle: S }),
  V: still({ ...fist, ...overRing, index: [9, 0, 0, 0], middle: [-9, 0, 0, 0] }),
  W: still({ ...fist, index: [10, 0, 0, 0], middle: S, ring: [-10, 0, 0, 0], touch: "pinky-finger-tip" }),
  X: still({ ...fist, index: [0, 15, 95, 75], rot: [0, 60, 0] }),
  Y: still({ ...fist, pinky: [-12, 0, 0, 0], thumb: [0, 60, 0, 0], touch: undefined }),
  Z: move({ ...fist, index: S }, { pos: [0.04, 0, 0] }, { pos: [0, -0.035, 0] }, { pos: [0.04, -0.035, 0] }),
};

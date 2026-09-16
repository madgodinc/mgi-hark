// Russian manual alphabet (дактильная азбука), 33 letters.
// Handshapes follow the per-letter teaching descriptions (opishi.me, "Дактиль —
// алфавит глухих") and were checked against the dactyl chart of the sign
// language linguistics lab (signlang.ru). Each letter is a list of keyframes:
// one keyframe is a still handshape, several make the letter a movement.
//
// Angles are explained in ../hand.js. pos moves the whole hand, in metres.

const S = [0, 0, 0, 0]; // straight finger
const F = [0, 88, 98, 62]; // finger folded into the fist
const THUMB_OVER = [42, 12, 22, 18]; // start for a thumb lying over folded fingers
const THUMB_SIDE = [0, 55, 0, 0]; // thumb stuck out to the side
const BENT90 = [0, 86, 0, 0]; // straight finger bent at the knuckle

const DOWN = { rot: [0, 180, 180] }; // hand hanging down, back of the hand to the viewer
const SIDE = { rot: [0, 75, 0] }; // thumb edge to the viewer: fingers bent forward read as pointing left

// Thumb and one finger closed into a ring, the other fingers straight.
const ringWith = (finger) => ({
  [finger]: [0, 55, 62, 40],
  thumb: [50, 30, 20, 10],
  touch: `${finger}-finger-tip`,
});

const fist = { index: F, middle: F, ring: F, pinky: F, thumb: THUMB_OVER, touch: "middle-finger-phalanx-intermediate" };
const overRing = { touch: "ring-finger-phalanx-intermediate" };
const bunch = {
  index: [-6, 45, 52, 22],
  middle: [0, 48, 52, 22],
  ring: [5, 50, 52, 22],
  pinky: [10, 52, 52, 22],
  thumb: [52, 30, 14, 10],
  touch: "middle-finger-tip",
};
const flatBent = { index: BENT90, middle: BENT90, ring: BENT90, pinky: BENT90 };

const still = (pose) => [pose];
const move = (pose, ...deltas) => [pose, ...deltas.map((d) => ({ ...pose, ...d }))];

export const RU = {
  А: still({ ...fist, thumb: [0, 0, 0, 0], touch: "index-finger-phalanx-intermediate", rot: [0, 50, 0] }),
  Б: still({ ...fist, ...overRing, index: S, middle: [0, 0, 18, 85] }),
  В: still({ thumb: [0, 20, 0, 0] }),
  Г: still({ ...fist, index: S, thumb: THUMB_SIDE, touch: undefined, rot: [0, -20, 160] }),
  Д: move({ ...fist, ...overRing, index: S, middle: S }, { pos: [0.012, 0.02, 0] }, { pos: [-0.01, 0.03, 0] }, { pos: [-0.004, -0.01, 0] }),
  Е: still({ ...bunch, ...SIDE }),
  Ё: move({ ...bunch, ...SIDE }, { rot: [0, 40, 0] }, { rot: [0, 95, 0] }, { rot: [0, 75, 0] }),
  Ж: still({ ...SIDE, ...flatBent, thumb: [30, 38, 0, 0], touch: "middle-finger-tip" }),
  З: move({ ...fist, index: S }, { pos: [0.03, 0.01, 0] }, { pos: [0, -0.015, 0] }, { pos: [0.03, -0.03, 0] }, { pos: [-0.005, -0.045, 0] }),
  И: still({ ...fist, ring: [4, 0, 0, 0], pinky: [-8, 0, 0, 0], touch: "index-finger-phalanx-intermediate" }),
  Й: move({ ...fist, ring: [4, 0, 0, 0], pinky: [-8, 0, 0, 0], touch: "index-finger-phalanx-intermediate" }, { rot: [0, 45, 0] }, { rot: [0, 0, 0] }),
  К: move({ ...fist, index: [7, 0, 0, 0], middle: [-5, 0, 0, 0], touch: "ring-finger-phalanx-intermediate" }, { rot: [35, 0, 0], pos: [0, -0.02, 0.02] }),
  Л: still({ ...fist, ...DOWN, ...overRing, index: [7, 0, 0, 0], middle: [-7, 0, 0, 0] }),
  М: still({ ...DOWN, index: [8, 0, 0, 0], ring: [-8, 0, 0, 0], ...ringWith("pinky") }),
  Н: still(ringWith("ring")),
  О: still({ ...ringWith("index"), index: [0, 45, 60, 40], rot: [0, 30, 0] }),
  П: still({ ...fist, ...DOWN, ...overRing, index: S, middle: S }),
  Р: still(ringWith("middle")),
  С: still({ rot: [0, 55, 0], index: [0, 30, 45, 20], middle: [0, 30, 45, 20], ring: [0, 30, 45, 20], pinky: [0, 30, 45, 20], thumb: [12, 40, 10, 0] }),
  Т: still({ ...DOWN, ...ringWith("pinky") }),
  У: still({ ...fist, pinky: S, thumb: THUMB_SIDE, touch: undefined }),
  Ф: still({ ...SIDE, ...flatBent, thumb: [0, 0, 0, 0], touch: "index-finger-phalanx-proximal" }),
  Х: still({ ...fist, index: [0, 25, 95, 70] }),
  Ц: move({ ...fist, ...overRing, index: S, middle: S }, { pos: [0, -0.05, 0] }),
  Ч: still({ ...fist, ...SIDE, index: BENT90, middle: BENT90, thumb: [30, 38, 0, 0], touch: "index-finger-tip" }),
  Ш: still(ringWith("pinky")),
  Щ: move(ringWith("pinky"), { pos: [0, -0.05, 0] }),
  Ъ: move({ ...fist, index: S, thumb: THUMB_SIDE, touch: undefined }, { rot: [0, 0, 18] }),
  Ы: still({ ...fist, ...overRing, index: S, pinky: S, rot: [0, 0, 90] }),
  Ь: move({ ...fist, index: S, thumb: THUMB_SIDE, touch: undefined }, { rot: [0, 0, -18] }),
  Э: still({ ...fist, index: [0, 35, 45, 20], thumb: [12, 40, 10, 0], touch: undefined, rot: [0, 55, 0] }),
  Ю: still({ rot: [0, 45, 0], index: BENT90, middle: BENT90, ring: BENT90, pinky: S, thumb: [30, 38, 0, 0], touch: "middle-finger-tip" }),
  Я: still({ ...fist, ...overRing, index: S, middle: [20, 0, 0, 0], rot: [0, 40, 0] }),
};

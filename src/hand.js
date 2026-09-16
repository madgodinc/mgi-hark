// A 3D right hand that takes fingerspelling poses.
//
// The model is the WebXR generic hand (MIT, Amazon). Its joints are flat
// siblings, not a chain, so bending a knuckle does not move the finger past
// it. We run forward kinematics ourselves: every joint keeps its rest offset
// from the previous one, expressed in that previous joint's frame, and a bend
// rotates everything further along the finger.

import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";

const FINGERS = ["index", "middle", "ring", "pinky"];
const CHAIN = (f) => [`${f}-finger-metacarpal`, `${f}-finger-phalanx-proximal`, `${f}-finger-phalanx-intermediate`, `${f}-finger-phalanx-distal`, `${f}-finger-tip`];
const THUMB = ["thumb-metacarpal", "thumb-phalanx-proximal", "thumb-phalanx-distal", "thumb-tip"];

const D = THREE.MathUtils.degToRad;

/**
 * A pose, all angles in degrees, every field optional (missing means rest):
 *   rot:    [pitch, yaw, roll] of the whole hand: pitch tips the fingers towards
 *           the viewer, yaw turns the palm away (positive shows the thumb
 *           edge), roll turns the hand in the picture plane counterclockwise
 *   index / middle / ring / pinky: [spread, knuckle, middle joint, end joint]
 *   thumb:  [fold across the palm, swing out of the palm plane, knuckle, end joint]
 * rot [0, 0, 0] shows the palm to the viewer with the fingers up.
 * Positive spread moves a finger towards the thumb.
 *   touch:  optional joint name, e.g. "index-finger-tip". The thumb angles are
 *           then solved so the thumb tip meets that joint; the listed thumb
 *           angles are only the starting guess.
 */
export const REST = {
  rot: [0, 0, 0],
  pos: [0, 0, 0],
  thumb: [0, 0, 0, 0],
  index: [0, 0, 0, 0],
  middle: [0, 0, 0, 0],
  ring: [0, 0, 0, 0],
  pinky: [0, 0, 0, 0],
};

export function full(pose) {
  return { ...REST, ...pose };
}

export function mix(a, b, t) {
  const out = {};
  for (const k of Object.keys(REST)) {
    out[k] = REST[k].map((_, i) => a[k][i] + (b[k][i] - a[k][i]) * t);
  }
  return out;
}

export class HandView {
  constructor(canvas, { color = "#f3efe6", outline = "#101010" } = {}) {
    this.canvas = canvas;
    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true, preserveDrawingBuffer: true });
    this.renderer.setPixelRatio(window.devicePixelRatio || 1);
    this.renderer.setClearColor(0x000000, 0);
    this.renderer.outputColorSpace = THREE.SRGBColorSpace;

    this.scene = new THREE.Scene();
    this.camera = new THREE.PerspectiveCamera(26, 1, 0.01, 10);
    this.camera.position.set(0, 0, 0.5);

    this.scene.add(new THREE.HemisphereLight(0xffffff, 0x8a8478, 2.2));
    const key = new THREE.DirectionalLight(0xffffff, 1.6);
    key.position.set(0.4, 0.8, 1);
    this.scene.add(key);

    this.holder = new THREE.Group(); // whole-hand orientation
    this.scene.add(this.holder);
    this.color = color;
    this.outline = outline;
    this.pose = full({});
  }

  async load(url) {
    const gltf = await new GLTFLoader().loadAsync(url);
    const root = gltf.scene;
    const mesh = root.getObjectByProperty("type", "SkinnedMesh");
    mesh.frustumCulled = false;
    mesh.material = new THREE.MeshToonMaterial({ color: this.color, gradientMap: toonRamp() });

    // Inverted-hull outline: the same skinned geometry, drawn from behind and
    // pushed outwards along the normals in the vertex shader.
    const shell = new THREE.SkinnedMesh(mesh.geometry, outlineMaterial(this.outline));
    shell.frustumCulled = false;
    shell.bind(mesh.skeleton, mesh.bindMatrix);
    mesh.parent.add(shell);

    this.joints = {};
    root.traverse((o) => {
      if (o.isBone || /finger|thumb|wrist/.test(o.name)) this.joints[o.name] = o;
    });
    this.rest = {};
    for (const [name, j] of Object.entries(this.joints)) {
      this.rest[name] = { p: j.position.clone(), q: j.quaternion.clone() };
    }

    // Center the hand on its palm so rotations turn it in place.
    const wrist = this.rest["wrist"].p;
    const knuckle = this.rest["middle-finger-phalanx-proximal"].p;
    const center = wrist.clone().lerp(knuckle, 0.7);
    root.position.sub(center);
    // Rest pose points the fingers along -Y; stand the hand up, palm to camera.
    this.base = new THREE.Group();
    this.base.add(root);
    this.base.rotation.set(0, 0, Math.PI);
    this.holder.add(this.base);

    this.setPose(this.pose);
    return this;
  }

  resize(w, h) {
    this.renderer.setSize(w, h, false);
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
  }

  chain(names, bends) {
    // bends[i] is a quaternion applied at joint i, in that joint's own frame.
    const r = names.map((n) => this.rest[n]);
    let pos = r[0].p.clone();
    let rot = r[0].q.clone().multiply(bends[0] || new THREE.Quaternion());
    this.place(names[0], pos, rot);
    for (let i = 1; i < names.length; i++) {
      const offset = r[i].p.clone().sub(r[i - 1].p).applyQuaternion(r[i - 1].q.clone().invert());
      pos = pos.clone().add(offset.applyQuaternion(rot));
      const relative = r[i - 1].q.clone().invert().multiply(r[i].q);
      rot = rot.clone().multiply(relative).multiply(bends[i] || new THREE.Quaternion());
      this.place(names[i], pos, rot);
    }
  }

  place(name, p, q) {
    const j = this.joints[name];
    j.position.copy(p);
    j.quaternion.copy(q);
  }

  setPose(pose) {
    if (pose.touch && this.joints) pose = this.solveTouch(pose);
    this.pose = pose;
    if (!this.joints) return;
    const [pitch, yaw, roll] = pose.rot;
    this.holder.rotation.set(D(roll), D(yaw - 90), D(pitch), "YXZ");
    this.holder.position.set(...pose.pos);

    const flex = (deg) => new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(1, 0, 0), D(-deg));
    const swing = (deg) => new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), D(deg));

    FINGERS.forEach((f) => {
      const [spread, k, m, e] = pose[f];
      this.chain(CHAIN(f), [null, swing(spread).multiply(flex(k)), flex(m), flex(e), null]);
    });
    const [fold, out, k, e] = pose.thumb;
    const base = swing(out).multiply(flex(fold));
    this.chain(THUMB, [base, flex(k), flex(e), null]);
  }

  /** Coordinate descent on the four thumb angles; results are cached per pose. */
  solveTouch(pose) {
    this.touchCache ||= new Map();
    const key = JSON.stringify(pose);
    if (this.touchCache.has(key)) return this.touchCache.get(key);
    const target = pose.touch;
    const limits = [[-40, 110], [-70, 90], [-20, 95], [-20, 95]];
    let thumb = [...pose.thumb];
    const tip = new THREE.Vector3();
    const goal = new THREE.Vector3();
    const cost = (t) => {
      this.setPose({ ...pose, touch: undefined, thumb: t });
      this.scene.updateMatrixWorld(true);
      this.joints["thumb-tip"].getWorldPosition(tip);
      this.joints[target].getWorldPosition(goal);
      // Small preference for relaxed joints keeps the solution natural.
      return tip.distanceTo(goal) + 0.00002 * (Math.abs(t[2]) + Math.abs(t[3]));
    };
    let best = cost(thumb);
    for (let step = 16; step >= 0.5; step /= 2) {
      let improved = true;
      while (improved) {
        improved = false;
        for (let i = 0; i < 4; i++) {
          for (const dir of [1, -1]) {
            const t = [...thumb];
            t[i] = Math.min(limits[i][1], Math.max(limits[i][0], t[i] + dir * step));
            const c = cost(t);
            if (c < best - 1e-7) {
              best = c;
              thumb = t;
              improved = true;
            }
          }
        }
      }
    }
    const solved = { ...pose, touch: undefined, thumb };
    this.touchCache.set(key, solved);
    return solved;
  }

  render() {
    this.renderer.render(this.scene, this.camera);
  }
}

function toonRamp() {
  const data = new Uint8Array([150, 150, 150, 255, 215, 215, 215, 255, 255, 255, 255, 255]);
  const tex = new THREE.DataTexture(data, 3, 1, THREE.RGBAFormat);
  tex.minFilter = tex.magFilter = THREE.NearestFilter;
  tex.needsUpdate = true;
  return tex;
}

function outlineMaterial(color) {
  const m = new THREE.MeshBasicMaterial({ color, side: THREE.BackSide });
  m.onBeforeCompile = (shader) => {
    shader.vertexShader = shader.vertexShader.replace(
      "#include <skinning_vertex>",
      "#include <skinning_vertex>\n  transformed += normalize(objectNormal) * 0.0016;",
    );
    shader.vertexShader = shader.vertexShader.replace("#include <beginnormal_vertex>", "#include <beginnormal_vertex>");
  };
  return m;
}

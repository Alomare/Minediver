<script>
  import { onMount } from "svelte";
  import * as THREE from "three";
  import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
  import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";

  let { url } = $props();
  let host = $state();
  let status = $state("Loading model");
  let wire = $state(false);
  let renderer, scene, camera, controls, frame, root, ro;
  let home = null;

  onMount(() => {
    renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    renderer.setPixelRatio(devicePixelRatio);
    renderer.outputColorSpace = THREE.SRGBColorSpace;
    host.appendChild(renderer.domElement);
    scene = new THREE.Scene();
    camera = new THREE.PerspectiveCamera(40, 1, 0.01, 1000);
    scene.add(new THREE.HemisphereLight(0xffffff, 0x404048, 2.2));
    const key = new THREE.DirectionalLight(0xffffff, 2.0);
    key.position.set(2, 3, 2);
    scene.add(key);
    const grid = new THREE.GridHelper(4, 16, 0x555b63, 0x33373d);
    grid.material.transparent = true;
    grid.material.opacity = 0.5;
    scene.add(grid);
    controls = new OrbitControls(camera, renderer.domElement);
    controls.enableDamping = true;
    const loop = () => {
      frame = requestAnimationFrame(loop);
      controls.update();
      renderer.render(scene, camera);
    };
    loop();
    ro = new ResizeObserver(() => {
      const w = host.clientWidth, h = host.clientHeight;
      renderer.setSize(w, h);
      camera.aspect = w / Math.max(1, h);
      camera.updateProjectionMatrix();
    });
    ro.observe(host);
    return () => {
      cancelAnimationFrame(frame);
      ro.disconnect();
      renderer.dispose();
    };
  });

  $effect(() => {
    const u = url;
    if (!scene) return;
    status = "Loading model";
    new GLTFLoader().load(
      u,
      (g) => {
        if (root) scene.remove(root);
        root = g.scene;
        root.traverse((o) => {
          if (o.isMesh) o.material.side = THREE.DoubleSide;
        });
        scene.add(root);
        const box = new THREE.Box3().setFromObject(root);
        const size = box.getSize(new THREE.Vector3()).length() || 1;
        const center = box.getCenter(new THREE.Vector3());
        controls.target.copy(center);
        camera.position.copy(center).add(new THREE.Vector3(size * 0.7, size * 0.45, size * 0.8));
        camera.near = size / 200;
        camera.far = size * 50;
        camera.updateProjectionMatrix();
        home = { pos: camera.position.clone(), target: controls.target.clone() };
        scene.children.find((c) => c.isGridHelper)?.scale.setScalar(Math.max(0.25, size / 2));
        status = "";
        applyWire();
      },
      undefined,
      () => (status = "This model could not be shown."),
    );
  });

  function resetView() {
    if (!home) return;
    camera.position.copy(home.pos);
    controls.target.copy(home.target);
    controls.update();
  }

  function applyWire() {
    root?.traverse((o) => {
      if (o.isMesh) o.material.wireframe = wire;
    });
  }
</script>

<div class="view" bind:this={host}>
  {#if status}<span class="status">{status}</span>{/if}
  <div class="tools">
    <button class="ghost" onclick={resetView}>Reset view</button>
    <button class="ghost" class:on={wire} onclick={() => { wire = !wire; applyWire(); }}>Wireframe</button>
  </div>
</div>

<style>
  .view { position: relative; width: 100%; height: 100%; }
  .view :global(canvas) { display: block; width: 100% !important; height: 100% !important; }
  .view { overflow: hidden; }
  .status { position: absolute; inset: 0; display: grid; place-items: center; color: var(--faint); }
  .tools { position: absolute; right: 6px; bottom: 6px; display: flex; gap: 4px; }
  .tools button { height: 24px; font-size: 12px; background: var(--panel); border-color: var(--line); }
  .tools button.on { border-color: var(--accent); color: var(--accent); }
</style>

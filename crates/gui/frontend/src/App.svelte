<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  // U2 脚手架：验证前端 ↔ Rust 后端桥接。后续单元替换为 Clean / Analyze / FDA 界面。
  let reply = $state("连接中…");
  invoke<string>("ping")
    .then((r) => (reply = r))
    .catch((e) => (reply = `失败: ${e}`));
</script>

<main>
  <h1>macCleaner</h1>
  <p>后端连接：{reply}</p>
</main>

<style>
  main {
    padding: 2rem;
  }
  h1 {
    font-size: 1.5rem;
    margin: 0 0 1rem;
  }
</style>

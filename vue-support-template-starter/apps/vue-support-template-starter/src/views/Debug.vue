<template>
  <div class="debug">
    <!-- 顶部组件信息条 -->
    <header class="debug__head">
      <div class="debug__back" @click="goHome">
        <el-icon><ArrowLeft /></el-icon>
        <span>返回组件库</span>
      </div>

      <div class="debug__info">
        <div class="debug__icon">
          <el-icon :size="20"><component :is="meta?.icon ?? "Box"" /></el-icon>
        </div>
        <div class="debug__title">
          <div class="debug__name">
            <span class="debug__title-cn">{{ meta?.title ?? name }}</span>
            <code class="debug__name-code">&lt;{{ name }} /&gt;</code>
            <el-tag
              v-if="hasSchema"
              size="small"
              type="success"
              effect="plain"
              round
            >
              结构化参数
            </el-tag>
            <el-tag v-else size="small" type="warning" effect="plain" round>通用调试</el-tag>
          </div>
          <div class="debug__desc">{{ meta?.description ?? "该组件尚未配置参数面板，使用通用调试模式" }}</div>
        </div>
      </div>

      <div class="debug__actions">
        <el-button :icon="Refresh" @click="reset">重置参数</el-button>
      </div>
    </header>

    <!-- 三栏布局：参数面板 | 预览区 | 代码区 -->
    <div class="debug__body" :class="{ 'debug__body--schema': hasSchema }">
      <!-- 左侧：参数面板 -->
      <aside class="debug__panel">
        <PropPanel
          v-if="hasSchema && templateMeta"
          :meta="templateMeta"
          :state="state"
          @update="onStateUpdate"
        />
        <RawPropsPanel
          v-else
          :component-name="name"
          :state="state"
          @update="onStateUpdate"
        />
      </aside>

      <!-- 中间：实时预览 -->
      <section class="debug__preview">
        <div class="debug__preview-head">
          <span>实时预览</span>
          <el-tag size="small" effect="plain" round>{{ name }}</el-tag>
        </div>
        <div class="debug__preview-body">
          <component
            :is="resolvedComponent"
            v-if="resolvedComponent && hasSchema"
            v-bind="vBindProps"
            v-model="state.model"
          >
            <template v-if="hasSchema" v-for="slot in templateMeta?.slots" :key="slot.name" #[slot.name]="slotProps">
              <component :is="resolveSlotText(state.slots[slot.name] ?? '')" v-bind="slotProps" />
            </template>
          </component>

          <!-- 通用调试：动态组件 + 透传 props -->
          <component
            :is="resolvedComponent"
            v-else-if="resolvedComponent"
            v-bind="rawProps"
            v-model="state.model"
          >
            <template v-for="(text, slotName) in state.slots" :key="slotName" #[slotName]>
              {{ text }}
            </template>
          </component>

          <el-empty v-else description="组件加载中..." />
        </div>
      </section>

      <!-- 右侧：代码区 -->
      <aside class="debug__code">
        <CodeBlock :code="code" :debug="debugCode" />
      </aside>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, defineAsyncComponent, ref, shallowRef, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { ArrowLeft, Refresh } from "@element-plus/icons-vue";
import type { Component } from "vue";
import PropPanel from "../components/PropPanel.vue";
import RawPropsPanel from "../components/RawPropsPanel.vue";
import CodeBlock from "../components/CodeBlock.vue";
import {
  templateMap,
  createInitialState,
  generateTemplateCode,
  formatPropsForDebug,
  type TemplateMeta,
  type TemplateState,
} from "../templates";
import { metaByName } from "../data/componentRegistry";

const route = useRoute();
const router = useRouter();

const name = computed(() => String(route.params.name ?? ""));
const meta = computed(() => metaByName.get(name.value));
const templateMeta = computed<TemplateMeta | undefined>(() => templateMap.get(name.value));
const hasSchema = computed(() => Boolean(templateMeta.value));

/** 调试状态 */
const state = ref<TemplateState>(initialState());

function initialState(): TemplateState {
  return templateMeta.value
    ? createInitialState(templateMeta.value)
    : {
        values: {},
        slots: { default: "" },
        model: false,
      };
}

/** 重置参数 */
const reset = () => {
  state.value = initialState();
};

const onStateUpdate = (next: TemplateState) => {
  state.value = next;
};

/** 解析默认插槽文本（支持换行 / 简单表达式） */
const resolveSlotText = (text: string) => {
  return { render: () => text };
};

/** 异步加载组件 */
const resolvedComponent = shallowRef<Component | null>(null);
watch(
  name,
  async (n) => {
    resolvedComponent.value = null;
    state.value = initialState();
    try {
      // 通过 import.meta.glob 异步加载对应路径
      const modules = import.meta.glob("../../packages/ui/src/components/*/index.ts");
      const path = `../../packages/ui/src/components/${n}/index.ts`;
      const loader = modules[path];
      if (!loader) return;
      const mod = await loader();
      resolvedComponent.value = (mod as { default?: Component }).default ?? null;
    } catch (e) {
      console.warn("[Debug] 组件加载失败", n, e);
    }
  },
  { immediate: true },
);

/** 结构化 props（v-bind 对象） */
const vBindProps = computed(() => {
  if (!templateMeta.value) return {};
  const out: Record<string, unknown> = {};
  for (const field of templateMeta.value.props) {
    if (field.isSlot) continue;
    out[field.key] = state.value.values[field.key];
  }
  return out;
});

/** 通用调试 props（来自 RawPropsPanel） */
const rawProps = computed(() => state.value.values);

/** 生成的 Vue 代码片段 */
const code = computed(() => {
  if (templateMeta.value) {
    return generateTemplateCode(templateMeta.value, state.value);
  }
  // 通用调试模式：直接输出 props JSON 形式的 ReXxx 标签
  const propsStr = Object.entries(state.value.values)
    .filter(([, v]) => v !== "" && v !== undefined && v !== null)
    .map(([k, v]) => {
      const value = typeof v === "string" ? `"${v}"` : JSON.stringify(v);
      return `${kebab(k)}=${value}`;
    })
    .join(" ");
  const vmodel = ` v-model="value"`;
  const slotLines = Object.entries(state.value.slots)
    .filter(([, text]) => text?.trim())
    .map(([slotName, text]) => (slotName === "default" ? `  ${text}` : `  <template #${slotName}>${text}</template>`))
    .join("\n");
  if (!propsStr && !slotLines) return `<${name.value} />`;
  if (!slotLines) return `<${name.value}${propsStr}${vmodel} />`;
  return `<${name.value}${propsStr}${vmodel}>\n${slotLines}\n</${name.value}>`;
});

/** 调试输出：Props JSON */
const debugCode = computed(() => {
  if (templateMeta.value) {
    return formatPropsForDebug(templateMeta.value, state.value);
  }
  return JSON.stringify(state.value, null, 2);
});

const kebab = (s: string) => s.replace(/([a-z0-9])([A-Z])/g, "$1-$2").toLowerCase();

const goHome = () => router.push("/home");
</script>

<style lang="scss" scoped>
.debug {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
}

.debug__head {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 14px 24px;
  background: var(--el-bg-color);
  border-bottom: 1px solid var(--el-border-color-lighter);
  flex-shrink: 0;
}

.debug__back {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px 10px;
  font-size: 13px;
  color: var(--el-text-color-secondary);
  cursor: pointer;
  border-radius: 6px;
  transition: all 0.2s ease;

  &:hover {
    color: #409eff;
    background: rgba(64, 158, 255, 0.08);
  }
}

.debug__info {
  display: flex;
  align-items: center;
  flex: 1;
  gap: 12px;
  min-width: 0;
}

.debug__icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  color: #fff;
  background: linear-gradient(135deg, #409eff 0%, #6f42c1 100%);
  border-radius: 10px;
  flex-shrink: 0;
}

.debug__title {
  min-width: 0;
  flex: 1;
}

.debug__name {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.debug__title-cn {
  font-size: 16px;
  font-weight: 700;
  color: var(--el-text-color-primary);
}

.debug__name-code {
  font-size: 12px;
  padding: 2px 8px;
  color: #5b8fd9;
  background: rgba(64, 158, 255, 0.08);
  border-radius: 4px;
}

.debug__desc {
  margin-top: 4px;
  font-size: 12px;
  color: var(--el-text-color-secondary);
  line-height: 1.4;
}

.debug__actions {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}

.debug__body {
  display: grid;
  grid-template-columns: 360px 1fr 1fr;
  gap: 12px;
  flex: 1;
  min-height: 0;
  padding: 12px 16px 16px;
  overflow: hidden;
}

.debug__body--schema {
  grid-template-columns: 340px 1fr 1fr;
}

.debug__panel {
  min-height: 0;
  overflow: hidden;
}

.debug__preview {
  display: flex;
  flex-direction: column;
  min-height: 0;
  background: linear-gradient(180deg, #f4f7fb 0%, #eaf1f9 100%);
  border: 1px solid var(--el-border-color-lighter);
  border-radius: 10px;
  overflow: hidden;
}

.debug__preview-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 14px;
  font-size: 13px;
  font-weight: 600;
  background: var(--el-bg-color);
  border-bottom: 1px solid var(--el-border-color-lighter);
  flex-shrink: 0;
}

.debug__preview-body {
  flex: 1;
  min-height: 0;
  padding: 24px;
  overflow: auto;
  display: flex;
  align-items: flex-start;
  justify-content: flex-start;
}

.debug__code {
  min-height: 0;
  overflow: hidden;
}
</style>
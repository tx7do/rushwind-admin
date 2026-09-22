<template>
  <div class="app-container h-full flex flex-1 flex-col">
    <el-card shadow="never">
      <template #header>
        <div class="card-header">
          <span class="card-title">{{ $t("pages.notification_channel.title") }}</span>
          <el-button type="primary" :icon="Plus" @click="openCreate">
            {{ $t("pages.notification_channel.create") }}
          </el-button>
        </div>
      </template>

      <el-table v-loading="isLoading" :data="items" border stripe row-key="id">
        <el-table-column :label="$t('pages.notification_channel.name')" prop="name" min-width="140" />
        <el-table-column :label="$t('pages.notification_channel.type')" width="110">
          <template #default="{ row }">
            <el-tag size="small" type="primary">{{ $t("pages.notification_channel.typeEmail") }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column :label="$t('pages.notification_channel.smtpHost')" prop="smtpHost" min-width="150" />
        <el-table-column :label="$t('pages.notification_channel.smtpPort')" prop="smtpPort" width="80" />
        <el-table-column :label="$t('pages.notification_channel.smtpFrom')" prop="smtpFrom" min-width="170" />
        <el-table-column :label="$t('pages.notification_channel.smtpTls')" width="110">
          <template #default="{ row }">{{ tlsLabel(row.smtpTls) }}</template>
        </el-table-column>
        <el-table-column :label="$t('pages.notification_channel.hasPassword')" width="100">
          <template #default="{ row }">
            <el-tag v-if="row.hasPassword" type="success" size="small">{{ $t("pages.notification_channel.passwordSet") }}</el-tag>
            <el-tag v-else size="small">{{ $t("pages.notification_channel.passwordNotSet") }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column :label="$t('pages.notification_channel.enabled')" width="90">
          <template #default="{ row }">
            <el-tag v-if="row.enabled" type="success" size="small">{{ $t("pages.notification_channel.enabledOn") }}</el-tag>
            <el-tag v-else size="small">{{ $t("pages.notification_channel.enabledOff") }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column :label="$t('common.table.action')" fixed="right" width="230">
          <template #default="{ row }">
            <el-button type="primary" link size="small" @click="openEdit(row)">
              {{ $t("common.button.edit") }}
            </el-button>
            <el-button type="primary" link size="small" @click="openTestSend(row)">
              {{ $t("pages.notification_channel.testSend") }}
            </el-button>
            <el-popconfirm :title="$t('pages.notification_channel.deleteConfirm')" @confirm="handleDelete(row)">
              <template #reference>
                <el-button type="danger" link size="small">
                  {{ $t("common.button.delete") }}
                </el-button>
              </template>
            </el-popconfirm>
          </template>
        </el-table-column>
      </el-table>

      <div class="pager">
        <el-pagination
          v-model:current-page="page"
          v-model:page-size="pageSize"
          :total="total"
          :page-sizes="[10, 20, 50]"
          layout="total, sizes, prev, pager, next"
          @current-change="load"
          @size-change="load"
        />
      </div>
    </el-card>

    <!-- 创建/编辑对话框 -->
    <el-dialog
      v-model="formOpen"
      :title="formMode === 'create' ? $t('pages.notification_channel.create') : $t('pages.notification_channel.edit')"
      width="520px"
      destroy-on-close
    >
      <el-form :model="form" label-width="140px">
        <el-form-item :label="$t('pages.notification_channel.name')" required>
          <el-input v-model="form.name" />
        </el-form-item>
        <el-form-item :label="$t('pages.notification_channel.type')">
          <el-select v-model="form.type" :disabled="formMode === 'edit'">
            <el-option :label="$t('pages.notification_channel.typeEmail')" value="EMAIL" />
          </el-select>
        </el-form-item>
        <el-form-item :label="$t('pages.notification_channel.smtpHost')">
          <el-input v-model="form.smtpHost" placeholder="smtp.example.com" />
        </el-form-item>
        <el-form-item :label="$t('pages.notification_channel.smtpPort')">
          <el-input-number v-model="form.smtpPort" :min="1" :max="65535" />
        </el-form-item>
        <el-form-item :label="$t('pages.notification_channel.smtpUsername')">
          <el-input v-model="form.smtpUsername" />
        </el-form-item>
        <el-form-item :label="$t('pages.notification_channel.password')">
          <el-input
            v-model="form.password"
            type="password"
            show-password
            :placeholder="formMode === 'edit' ? $t('pages.notification_channel.passwordKeepHint') : $t('pages.notification_channel.passwordPlaceholder')"
          />
        </el-form-item>
        <el-form-item :label="$t('pages.notification_channel.smtpFrom')">
          <el-input v-model="form.smtpFrom" placeholder="noreply@example.com" />
        </el-form-item>
        <el-form-item :label="$t('pages.notification_channel.smtpTls')">
          <el-select v-model="form.smtpTls">
            <el-option :label="$t('pages.notification_channel.tlsNone')" value="NONE" />
            <el-option :label="$t('pages.notification_channel.tlsStartTls')" value="START_TLS" />
            <el-option :label="$t('pages.notification_channel.tlsSsl')" value="SSL" />
          </el-select>
        </el-form-item>
        <el-form-item :label="$t('pages.notification_channel.enabled')">
          <el-switch v-model="form.enabled" />
        </el-form-item>
        <el-form-item :label="$t('pages.notification_channel.remark')">
          <el-input v-model="form.remark" type="textarea" :rows="2" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="formOpen = false">{{ $t("common.button.cancel") }}</el-button>
        <el-button type="primary" :loading="saving" @click="handleSave">
          {{ $t("common.button.confirm") }}
        </el-button>
      </template>
    </el-dialog>

    <!-- 测试发送对话框 -->
    <el-dialog
      v-model="testOpen"
      :title="$t('pages.notification_channel.testSendTitle', { name: testTarget?.name || '' })"
      width="420px"
      destroy-on-close
    >
      <el-form label-width="140px">
        <el-form-item :label="$t('pages.notification_channel.testRecipient')" required>
          <el-input v-model="testRecipient" placeholder="you@example.com" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="testOpen = false">{{ $t("common.button.cancel") }}</el-button>
        <el-button type="primary" :loading="testing" @click="handleTestSend">
          {{ $t("pages.notification_channel.testSend") }}
        </el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script lang="ts" setup>
import { onMounted, ref } from "vue";
import { Plus } from "@element-plus/icons-vue";
import { ElMessage } from "element-plus";

import type { notification_channelservicev1_NotificationChannel as NotificationChannel } from "@/api/generated/admin/service/v1";
import {
  fetchListNotificationChannels,
  useCreateNotificationChannel,
  useDeleteNotificationChannel,
  useSendTestEmail,
  useUpdateNotificationChannel,
} from "@/api/composables";
import { $t } from "@/core/i18n";

const { mutateAsync: createChannel } = useCreateNotificationChannel();
const { mutateAsync: updateChannel } = useUpdateNotificationChannel();
const { mutateAsync: deleteChannel } = useDeleteNotificationChannel();
const { mutateAsync: sendTest } = useSendTestEmail();

const isLoading = ref(false);
const items = ref<NotificationChannel[]>([]);
const total = ref(0);
const page = ref(1);
const pageSize = ref(20);

async function load() {
  isLoading.value = true;
  try {
    const resp = await fetchListNotificationChannels({
      page: page.value,
      pageSize: pageSize.value,
    });
    items.value = (resp.items ?? []) as NotificationChannel[];
    total.value = resp.total ?? 0;
  } catch (error: any) {
    ElMessage.error(error?.message || $t("pages.notification_channel.fetchFailed"));
  } finally {
    isLoading.value = false;
  }
}

onMounted(load);

const formOpen = ref(false);
const formMode = ref<"create" | "edit">("create");
const saving = ref(false);
const editingId = ref<number>();
const form = ref({
  name: "",
  type: "EMAIL",
  smtpHost: "",
  smtpPort: 587,
  smtpUsername: "",
  password: "",
  smtpFrom: "",
  smtpTls: "START_TLS",
  enabled: true,
  remark: "",
});

function openCreate() {
  formMode.value = "create";
  editingId.value = undefined;
  form.value = {
    name: "", type: "EMAIL", smtpHost: "", smtpPort: 587, smtpUsername: "",
    password: "", smtpFrom: "", smtpTls: "START_TLS", enabled: true, remark: "",
  };
  formOpen.value = true;
}

function openEdit(row: NotificationChannel) {
  formMode.value = "edit";
  editingId.value = row.id;
  form.value = {
    name: row.name || "",
    type: row.type || "EMAIL",
    smtpHost: row.smtpHost || "",
    smtpPort: row.smtpPort ?? 587,
    smtpUsername: row.smtpUsername || "",
    password: "",
    smtpFrom: row.smtpFrom || "",
    smtpTls: row.smtpTls || "START_TLS",
    enabled: !!row.enabled,
    remark: row.remark || "",
  };
  formOpen.value = true;
}

async function handleSave() {
  if (!form.value.name) {
    ElMessage.error($t("pages.notification_channel.requiredName"));
    return;
  }
  saving.value = true;
  try {
    const { password, ...data } = form.value as Record<string, any>;
    if (formMode.value === "create") {
      if (!password) {
        ElMessage.error($t("pages.notification_channel.requiredPassword"));
        return;
      }
      await createChannel({ data, password });
      ElMessage.success($t("pages.notification_channel.createSuccess"));
    } else {
      await updateChannel({
        id: editingId.value,
        data,
        password: password || undefined,
        updateMask:
          "name,smtpHost,smtpPort,smtpUsername,smtpFrom,smtpTls,enabled,remark",
      });
      ElMessage.success($t("pages.notification_channel.updateSuccess"));
    }
    formOpen.value = false;
    await load();
  } catch (error: any) {
    ElMessage.error(error?.message || $t("pages.notification_channel.saveFailed"));
  } finally {
    saving.value = false;
  }
}

async function handleDelete(row: NotificationChannel) {
  if (!row.id) return;
  try {
    await deleteChannel({ id: row.id });
    ElMessage.success($t("pages.notification_channel.deleteSuccess"));
    await load();
  } catch (error: any) {
    ElMessage.error(error?.message || $t("pages.notification_channel.deleteFailed"));
  }
}

const testOpen = ref(false);
const testing = ref(false);
const testTarget = ref<NotificationChannel>();
const testRecipient = ref("");

function openTestSend(row: NotificationChannel) {
  testTarget.value = row;
  testRecipient.value = "";
  testOpen.value = true;
}

async function handleTestSend() {
  if (!testTarget.value?.id || !testRecipient.value) return;
  testing.value = true;
  try {
    await sendTest({ id: testTarget.value.id, recipient: testRecipient.value });
    ElMessage.success($t("pages.notification_channel.testSendSuccess"));
    testOpen.value = false;
  } catch (error: any) {
    ElMessage.error(error?.message || $t("pages.notification_channel.testSendFailed"));
  } finally {
    testing.value = false;
  }
}

function tlsLabel(mode?: string): string {
  const map: Record<string, string> = {
    NONE: $t("pages.notification_channel.tlsNone"),
    START_TLS: $t("pages.notification_channel.tlsStartTls"),
    SSL: $t("pages.notification_channel.tlsSsl"),
  };
  return (mode && map[mode]) || mode || "-";
}
</script>

<style lang="scss" scoped>
.card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.card-title {
  font-weight: 600;
}
.pager {
  margin-top: 12px;
  display: flex;
  justify-content: flex-end;
}
</style>

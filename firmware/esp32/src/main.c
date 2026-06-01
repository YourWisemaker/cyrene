/**
 * Cyrene companion firmware for ESP32 (R37).
 *
 * Implements the host↔firmware protocol:
 *   - JSON-RPC 2.0 over serial (115200 baud)
 *   - Methods: ping, read_pin, write_pin, read_i2c, write_i2c, version
 *   - Protocol version: 1
 *
 * Build: idf.py build && idf.py flash
 * Monitor: idf.py monitor
 */

#include <stdio.h>
#include <string.h>
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "driver/gpio.h"
#include "driver/i2c.h"
#include "esp_system.h"
#include "esp_log.h"
#include "cJSON.h"

static const char *TAG = "cyrene-fw";
#define PROTOCOL_VERSION 1
#define UART_BUF_SIZE 1024

static esp_err_t i2c_master_init(void) {
    i2c_config_t conf = {
        .mode = I2C_MODE_MASTER,
        .sda_io_num = 21,
        .scl_io_num = 22,
        .sda_pullup_en = GPIO_PULLUP_ENABLE,
        .scl_pullup_en = GPIO_PULLUP_ENABLE,
        .master.clk_speed = 100000,
    };
    esp_err_t err = i2c_param_config(I2C_NUM_0, &conf);
    if (err != ESP_OK) return err;
    return i2c_driver_install(I2C_NUM_0, conf.mode, 0, 0, 0);
}

static cJSON* handle_ping(const cJSON *id) {
    cJSON *resp = cJSON_CreateObject();
    cJSON_AddStringToObject(resp, "jsonrpc", "2.0");
    cJSON_AddItemToObject(resp, "id", cJSON_Duplicate(id, 1));
    cJSON *result = cJSON_CreateObject();
    cJSON_AddNumberToObject(result, "protocol_version", PROTOCOL_VERSION);
    cJSON_AddNumberToObject(result, "uptime_ms", (double)xTaskGetTickCount() * portTICK_PERIOD_MS);
    cJSON_AddItemToObject(resp, "result", result);
    return resp;
}

static cJSON* handle_version(const cJSON *id) {
    cJSON *resp = cJSON_CreateObject();
    cJSON_AddStringToObject(resp, "jsonrpc", "2.0");
    cJSON_AddItemToObject(resp, "id", cJSON_Duplicate(id, 1));
    cJSON *result = cJSON_CreateObject();
    cJSON_AddNumberToObject(result, "protocol_version", PROTOCOL_VERSION);
    cJSON_AddStringToObject(result, "firmware", "cyrene-esp32-0.1.0");
    cJSON_AddStringToObject(result, "chip", CONFIG_IDF_TARGET);
    cJSON_AddItemToObject(resp, "result", result);
    return resp;
}

static cJSON* handle_read_pin(const cJSON *id, const cJSON *params) {
    cJSON *resp = cJSON_CreateObject();
    cJSON_AddStringToObject(resp, "jsonrpc", "2.0");
    cJSON_AddItemToObject(resp, "id", cJSON_Duplicate(id, 1));

    const cJSON *pin_json = cJSON_GetObjectItem(params, "pin");
    if (!pin_json || !cJSON_IsNumber(pin_json)) {
        cJSON *err = cJSON_CreateObject();
        cJSON_AddNumberToObject(err, "code", -32602);
        cJSON_AddStringToObject(err, "message", "Invalid params: 'pin' required");
        cJSON_AddItemToObject(resp, "error", err);
        return resp;
    }

    int pin = pin_json->valueint;
    gpio_set_direction(pin, GPIO_MODE_INPUT);
    int val = gpio_get_level(pin);

    cJSON *result = cJSON_CreateObject();
    cJSON_AddNumberToObject(result, "pin", pin);
    cJSON_AddNumberToObject(result, "value", val);
    cJSON_AddItemToObject(resp, "result", result);
    return resp;
}

static cJSON* handle_write_pin(const cJSON *id, const cJSON *params) {
    cJSON *resp = cJSON_CreateObject();
    cJSON_AddStringToObject(resp, "jsonrpc", "2.0");
    cJSON_AddItemToObject(resp, "id", cJSON_Duplicate(id, 1));

    const cJSON *pin_json = cJSON_GetObjectItem(params, "pin");
    const cJSON *val_json = cJSON_GetObjectItem(params, "value");
    if (!pin_json || !cJSON_IsNumber(pin_json) || !val_json || !cJSON_IsNumber(val_json)) {
        cJSON *err = cJSON_CreateObject();
        cJSON_AddNumberToObject(err, "code", -32602);
        cJSON_AddStringToObject(err, "message", "Invalid params: 'pin' and 'value' required");
        cJSON_AddItemToObject(resp, "error", err);
        return resp;
    }

    int pin = pin_json->valueint;
    int val = val_json->valueint;
    gpio_set_direction(pin, GPIO_MODE_OUTPUT);
    gpio_set_level(pin, val);

    cJSON *result = cJSON_CreateObject();
    cJSON_AddNumberToObject(result, "pin", pin);
    cJSON_AddNumberToObject(result, "value", val);
    cJSON_AddBoolToObject(result, "success", 1);
    cJSON_AddItemToObject(resp, "result", result);
    return resp;
}

static cJSON* handle_request(const char *json_str) {
    cJSON *req = cJSON_Parse(json_str);
    if (!req) {
        cJSON *resp = cJSON_CreateObject();
        cJSON_AddStringToObject(resp, "jsonrpc", "2.0");
        cJSON_AddNullToObject(resp, "id");
        cJSON *err = cJSON_CreateObject();
        cJSON_AddNumberToObject(err, "code", -32700);
        cJSON_AddStringToObject(err, "message", "Parse error");
        cJSON_AddItemToObject(resp, "error", err);
        return resp;
    }

    const cJSON *method = cJSON_GetObjectItem(req, "method");
    const cJSON *id = cJSON_GetObjectItem(req, "id");
    const cJSON *params = cJSON_GetObjectItem(req, "params");

    cJSON *null_id = cJSON_CreateNull();
    const cJSON *effective_id = id ? id : null_id;

    cJSON *resp = NULL;
    if (method && cJSON_IsString(method)) {
        if (strcmp(method->valuestring, "ping") == 0) {
            resp = handle_ping(effective_id);
        } else if (strcmp(method->valuestring, "version") == 0) {
            resp = handle_version(effective_id);
        } else if (strcmp(method->valuestring, "read_pin") == 0) {
            resp = handle_read_pin(effective_id, params);
        } else if (strcmp(method->valuestring, "write_pin") == 0) {
            resp = handle_write_pin(effective_id, params);
        }
    }

    if (!resp) {
        resp = cJSON_CreateObject();
        cJSON_AddStringToObject(resp, "jsonrpc", "2.0");
        cJSON_AddItemToObject(resp, "id", cJSON_Duplicate(effective_id, 1));
        cJSON *err = cJSON_CreateObject();
        cJSON_AddNumberToObject(err, "code", -32601);
        cJSON_AddStringToObject(err, "message", "Method not found");
        cJSON_AddItemToObject(resp, "error", err);
    }

    cJSON_Delete(null_id);
    cJSON_Delete(req);
    return resp;
}

void app_main(void) {
    ESP_LOGI(TAG, "Cyrene firmware starting (protocol v%d)", PROTOCOL_VERSION);
    i2c_master_init();

    char buf[UART_BUF_SIZE];
    size_t pos = 0;

    while (1) {
        int c = fgetc(stdin);
        if (c == EOF) {
            vTaskDelay(pdMS_TO_TICKS(10));
            continue;
        }
        if (c == '\n') {
            buf[pos] = '\0';
            if (pos > 0) {
                cJSON *resp = handle_request(buf);
                char *out = cJSON_PrintUnformatted(resp);
                printf("%s\n", out);
                free(out);
                cJSON_Delete(resp);
            }
            pos = 0;
        } else if (pos < UART_BUF_SIZE - 1) {
            buf[pos++] = (char)c;
        }
    }
}

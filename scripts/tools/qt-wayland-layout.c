// Private-seat fixture for the standard virtual-keyboard-unstable-v1 protocol.
// Sends Linux physical Q (evdev 16) under a fixed French keymap, without wtype's
// dynamically assigned keycodes. Used only by qt-native-layout-private.py.
#define _GNU_SOURCE
#include <wayland-client.h>
#include <xkbcommon/xkbcommon.h>
#include <sys/mman.h>
#include <unistd.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

static const struct wl_interface keyboard_interface;
static const struct wl_interface *create_types[] = {&wl_seat_interface,&keyboard_interface};
static const struct wl_message manager_requests[] = {{"create_virtual_keyboard","on",create_types}};
static const struct wl_interface manager_interface = {"zwp_virtual_keyboard_manager_v1",1,1,manager_requests,0,NULL};
static const struct wl_message keyboard_requests[] = {
    {"keymap","uhu",NULL},{"key","uuu",NULL},{"modifiers","uuuu",NULL},{"destroy","",NULL}};
static const struct wl_interface keyboard_interface = {"zwp_virtual_keyboard_v1",1,4,keyboard_requests,0,NULL};
static struct wl_proxy *manager;
static struct wl_seat *seat;
static void global(void *data,struct wl_registry *registry,uint32_t id,const char *name,uint32_t version) {
    (void)data;(void)version;
    if(!strcmp(name,"zwp_virtual_keyboard_manager_v1"))manager=wl_registry_bind(registry,id,&manager_interface,1);
    if(!strcmp(name,"wl_seat"))seat=wl_registry_bind(registry,id,&wl_seat_interface,1);
}
static void removed(void *data,struct wl_registry *registry,uint32_t id){(void)data;(void)registry;(void)id;}
int main(void) {
    if(!getenv("REX_QT_NATIVE_LAYOUT_CHECK"))return 2;
    struct wl_display *display=wl_display_connect(NULL);if(!display)return 3;
    struct wl_registry *registry=wl_display_get_registry(display);
    const struct wl_registry_listener listener={global,removed};wl_registry_add_listener(registry,&listener,NULL);
    if(wl_display_roundtrip(display)<0 || !manager || !seat)return 4;
    struct xkb_context *context=xkb_context_new(XKB_CONTEXT_NO_FLAGS);
    const struct xkb_rule_names names={.layout="fr"};
    struct xkb_keymap *map=xkb_keymap_new_from_names(context,&names,XKB_KEYMAP_COMPILE_NO_FLAGS);
    if(!map)return 5;
    char *text=xkb_keymap_get_as_string(map,XKB_KEYMAP_FORMAT_TEXT_V1);size_t size=strlen(text)+1;
    int fd=memfd_create("private-layout",MFD_CLOEXEC);if(fd<0 || write(fd,text,size)!=(ssize_t)size)return 6;
    struct wl_proxy *keyboard=wl_proxy_marshal_flags(manager,0,&keyboard_interface,1,0,seat,NULL);
    wl_proxy_marshal_flags(keyboard,0,NULL,1,0,1,fd,(uint32_t)size);
    wl_display_roundtrip(display);usleep(400000);
    wl_proxy_marshal_flags(keyboard,1,NULL,1,0,1,16,1);
    wl_proxy_marshal_flags(keyboard,1,NULL,1,0,2,16,0);
    wl_display_roundtrip(display);usleep(100000);
    wl_proxy_marshal_flags(keyboard,3,NULL,1,WL_MARSHAL_FLAG_DESTROY);
    close(fd);free(text);xkb_keymap_unref(map);xkb_context_unref(context);wl_display_disconnect(display);
    return 0;
}

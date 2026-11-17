// SPDX-License-Identifier: GPL-3.0-only
#import <AppKit/AppKit.h>
#import <UserNotifications/UserNotifications.h>

// All mutable notification state belongs to the main queue. Requests are uniquely
// identified so late permission/delivery callbacks cannot resurrect an old prompt
// or remove a newer prompt. They never carry bank data or trigger a bank action.
static UNUserNotificationCenter *center;
static NSMutableDictionary<NSString *, NSDictionary *> *active;
static id<UNUserNotificationCenterDelegate> delegate;
static void onMain(dispatch_block_t block) {
    if ([NSThread isMainThread]) block(); else dispatch_async(dispatch_get_main_queue(), block);
}
static BOOL current(NSString *key, NSDictionary *entry) {
    return active[key] == entry && (!entry[@"expires"] || [entry[@"expires"] timeIntervalSinceNow] > 0);
}
static void clear(NSString *key) {
    NSString *identifier = active[key][@"id"];
    [active removeObjectForKey:key];
    if (identifier) {
        [center removePendingNotificationRequestsWithIdentifiers:@[identifier]];
        [center removeDeliveredNotificationsWithIdentifiers:@[identifier]];
    }
}
static void deliver(NSString *key, NSDictionary *entry) {
    if (!center || !current(key,entry)) return;
    [center getNotificationSettingsWithCompletionHandler:^(UNNotificationSettings *settings) {
        onMain(^{
            if (!current(key,entry) || [entry[@"submitted"] boolValue] || settings.authorizationStatus != UNAuthorizationStatusAuthorized) return;
            [(NSMutableDictionary *)entry setObject:@YES forKey:@"submitted"];
            UNMutableNotificationContent *content = [UNMutableNotificationContent new];
            content.title = entry[@"title"];
            content.body = entry[@"body"];
            content.threadIdentifier = @"bankingcraft.activity";
            content.userInfo = @{@"kind":key};
            // Bundled low-amplitude PCM chime. Do not substitute a loud system beep
            // or bypass Focus / the user's notification and sound preferences.
            if ([[NSBundle mainBundle] URLForResource:@"bankingcraft-gentle" withExtension:@"wav"])
                content.sound = [UNNotificationSound soundNamed:@"bankingcraft-gentle.wav"];
            UNNotificationRequest *request = [UNNotificationRequest requestWithIdentifier:entry[@"id"] content:content trigger:nil];
            [center addNotificationRequest:request withCompletionHandler:^(NSError *error) {
                onMain(^{
                    if (!current(key,entry)) {
                        [center removePendingNotificationRequestsWithIdentifiers:@[request.identifier]];
                        [center removeDeliveredNotificationsWithIdentifiers:@[request.identifier]];
                    }
                    if (error) NSLog(@"BankingCraft: local notification could not be delivered");
                });
            }];
        });
    }];
}
static void publish(NSString *key, NSString *title, NSString *body, unsigned lifetime) {
    if (!center) return;
    clear(key);
    NSMutableDictionary *entry = [@{@"id":[NSString stringWithFormat:@"bankingcraft.%@.%@",key,[NSUUID UUID].UUIDString],
                                   @"title":title,@"body":body} mutableCopy];
    if ([key isEqualToString:@"reader"]) entry[@"expires"] = [NSDate dateWithTimeIntervalSinceNow:lifetime];
    active[key] = entry;
    deliver(key,entry);
    if ([key isEqualToString:@"reader"]) {
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW,(int64_t)lifetime*NSEC_PER_SEC),dispatch_get_main_queue(),^{
            if (active[key]==entry) clear(key);
        });
    }
}
@interface BankingCraftNotifications : NSObject <UNUserNotificationCenterDelegate>
@end
@implementation BankingCraftNotifications
- (void)userNotificationCenter:(UNUserNotificationCenter *)unused willPresentNotification:(UNNotification *)notification
        withCompletionHandler:(void (^)(UNNotificationPresentationOptions))completion {
    (void)unused;
    onMain(^{
        NSString *key=notification.request.content.userInfo[@"kind"];
        NSDictionary *entry=key ? active[key] : nil;
        BOOL valid=entry && current(key,entry) && [entry[@"id"] isEqualToString:notification.request.identifier];
        completion(valid ? UNNotificationPresentationOptionBanner|UNNotificationPresentationOptionList|UNNotificationPresentationOptionSound : UNNotificationPresentationOptionNone);
    });
}
- (void)userNotificationCenter:(UNUserNotificationCenter *)unused didReceiveNotificationResponse:(UNNotificationResponse *)response
        withCompletionHandler:(void (^)(void))completion {
    (void)unused; (void)response;
    onMain(^{ [NSApp activateIgnoringOtherApps:YES]; completion(); });
}
@end

void bankingcraft_notifications_init(const char *titleUTF8,const char *bodyUTF8) {
    NSString *title=[NSString stringWithUTF8String:titleUTF8];
    NSString *body=[NSString stringWithUTF8String:bodyUTF8];
    onMain(^{
        if (center) return;
        // UNUserNotificationCenter requires an app bundle. Plain `cargo run` and
        // command-line test binaries must not crash or request OS permission.
        NSBundle *bundle=[NSBundle mainBundle];
        if (![bundle.bundleIdentifier isEqualToString:@"de.appcreation.bankingcraft"] || ![bundle.bundlePath hasSuffix:@".app"]) return;
        center=[UNUserNotificationCenter currentNotificationCenter];
        active=[NSMutableDictionary new];
        delegate=[BankingCraftNotifications new]; center.delegate=delegate;
        [center getDeliveredNotificationsWithCompletionHandler:^(NSArray<UNNotification *> *notifications) {
            onMain(^{
                NSMutableArray *oldReaders=[NSMutableArray new];
                for (UNNotification *n in notifications) {
                    if ([n.request.identifier hasPrefix:@"bankingcraft.reader."] && ![active[@"reader"][@"id"] isEqualToString:n.request.identifier]) [oldReaders addObject:n.request.identifier];
                }
                [center removeDeliveredNotificationsWithIdentifiers:oldReaders];
            });
        }];
        [center getNotificationSettingsWithCompletionHandler:^(UNNotificationSettings *settings) {
            if (settings.authorizationStatus != UNAuthorizationStatusNotDetermined) return;
            [center requestAuthorizationWithOptions:UNAuthorizationOptionAlert|UNAuthorizationOptionSound
                completionHandler:^(BOOL granted,NSError *error) {
                    (void)error;
                    if (!granted) return;
                    onMain(^{
                        // One quiet confirmation when the user first enables alerts.
                        if (active.count==0) publish(@"enabled",title,body,0);
                        else for (NSString *key in [active.allKeys copy]) deliver(key,active[key]);
                    });
                }];
        }];
    });
}
void bankingcraft_notifications_post(bool reader,const char *titleUTF8,const char *bodyUTF8,unsigned lifetime) {
    NSString *title=[NSString stringWithUTF8String:titleUTF8];
    NSString *body=[NSString stringWithUTF8String:bodyUTF8];
    onMain(^{ publish(reader ? @"reader" : @"sync",title,body,lifetime); });
}
void bankingcraft_notifications_clear_reader(void) { onMain(^{ clear(@"reader"); }); }

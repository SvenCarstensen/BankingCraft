// SPDX-License-Identifier: GPL-3.0-only
#import <AppKit/AppKit.h>

// Observers live as long as the application. They never unlock a session.
void bankingcraft_observe_session(void (*expire)(void)) {
    static NSMutableArray *observers;
    if (observers) return;
    observers = [NSMutableArray new];
    NSNotificationCenter *workspace = [[NSWorkspace sharedWorkspace] notificationCenter];
    for (NSNotificationName name in @[NSWorkspaceWillSleepNotification,
                                       NSWorkspaceScreensDidSleepNotification,
                                       NSWorkspaceSessionDidResignActiveNotification]) {
        [observers addObject:[workspace addObserverForName:name object:nil queue:nil
                                               usingBlock:^(NSNotification *note) { (void)note; expire(); }]];
    }
    // macOS distributes this lock notification in addition to the documented
    // workspace sleep/session notifications. Treat it solely as a lock request.
    [observers addObject:[[NSDistributedNotificationCenter defaultCenter]
        addObserverForName:@"com.apple.screenIsLocked" object:nil queue:nil
        usingBlock:^(NSNotification *note) { (void)note; expire(); }]];
}

@0xaff784be6017f80e;

interface BlockerService {
    registerLogger @0 (logger :Logger);
    setRuleset @1 (hook :FilterHook, ruleset :FilterRuleset);
    enableFiltering @2 ();
    disableFiltering @3 ();

    enum FilterHook {
        getAddrInfo @0;
        cefUrlRequestCreate @1;
    }

    struct FilterRuleset {
        whitelist @0 :List(Text);
        blacklist @1 :List(Text);
    }

    interface Logger {
        struct Request {
            url @0 :Text;
            hook @1 :FilterHook;
            blocked @2 :Bool;
        }

        logRequest @0 (request :Request);
        logMessage @1 (message :Text);
    }
}

// Generated from `schema.sql` by its test. Do not edit.

diesel::table! {
    agent (id) {
        id -> BigInt,
        kind -> Text,
        since -> BigInt,
        harness -> Text,
        effort -> Nullable<Text>,
        fast -> Bool,
        ended_at -> Nullable<BigInt>,
        ended_by -> Nullable<Text>,
        ended_how -> Nullable<Text>,
        ended_reason -> Nullable<Text>,
        erased -> Bool,
    }
}

diesel::table! {
    agreement (id) {
        id -> BigInt,
        a -> BigInt,
        b -> BigInt,
        hamlet -> Text,
        mayor -> BigInt,
        at -> BigInt,
        reason -> Text,
        task -> Nullable<BigInt>,
        road -> Nullable<BigInt>,
    }
}

diesel::table! {
    answer_fault (conversation, input) {
        conversation -> BigInt,
        input -> Text,
        words -> Text,
        reported -> Bool,
    }
}

diesel::table! {
    attempt (runtime, key) {
        runtime -> Text,
        key -> Text,
        conversation -> BigInt,
        agent -> BigInt,
        payer -> Nullable<Text>,
        task -> Nullable<BigInt>,
        provider -> Text,
        model -> Text,
        price -> Text,
        at -> BigInt,
    }
}

diesel::table! {
    budget_reset (id) {
        id -> BigInt,
        hamlet -> Nullable<Text>,
        at -> BigInt,
        chief -> BigInt,
        usage -> BigInt,
        road_charge -> BigInt,
    }
}

diesel::table! {
    chat (agent, chief) {
        agent -> BigInt,
        chief -> BigInt,
        current -> BigInt,
        reflections -> BigInt,
    }
}

diesel::table! {
    chat_session (agent, chief, session) {
        agent -> BigInt,
        chief -> BigInt,
        session -> BigInt,
        conversation -> BigInt,
        began -> BigInt,
    }
}

diesel::table! {
    chief (id) {
        id -> BigInt,
        name -> Text,
        since -> BigInt,
    }
}

diesel::table! {
    conversation (id) {
        id -> BigInt,
        agent -> BigInt,
        kind -> Text,
        chief -> Nullable<BigInt>,
        session -> Nullable<BigInt>,
        peer -> Nullable<BigInt>,
        errand -> Nullable<Text>,
        owner -> Nullable<Text>,
        instructions -> Nullable<Text>,
        runtime -> Text,
        handle -> Nullable<Text>,
        adapter -> Nullable<Text>,
        version -> Nullable<Text>,
        model -> Text,
        fork_of -> Nullable<BigInt>,
        fork_at -> Nullable<Text>,
        previous -> Nullable<BigInt>,
        tools -> Nullable<Text>,
        tools_told -> Nullable<Text>,
        cancelling -> Bool,
        stopped -> Bool,
        last -> Nullable<Text>,
        unreflected -> BigInt,
        retiring -> Bool,
        entered -> Nullable<Text>,
        last_input -> Nullable<Text>,
        made -> BigInt,
    }
}

diesel::table! {
    decision (id) {
        id -> BigInt,
        at -> BigInt,
        hamlet -> Nullable<Text>,
        actor -> Text,
        kind -> Text,
        subject -> Nullable<BigInt>,
        other -> Nullable<BigInt>,
        scholar -> Nullable<BigInt>,
        reason -> Text,
        task -> Nullable<BigInt>,
        detail -> Nullable<Text>,
    }
}

diesel::table! {
    document (kind) {
        kind -> Text,
        value -> Text,
        at -> BigInt,
        chief -> Nullable<BigInt>,
    }
}

diesel::table! {
    dream (id) {
        id -> BigInt,
        kind -> Text,
        scholar -> BigInt,
        hire -> Nullable<BigInt>,
        ending -> Bool,
        conversation -> Nullable<BigInt>,
        task -> Nullable<BigInt>,
        began -> BigInt,
        ended -> Nullable<BigInt>,
        outcome -> Nullable<Text>,
        main_commit -> Nullable<Text>,
        branch_commit -> Nullable<Text>,
    }
}

diesel::table! {
    hamlet (id) {
        id -> Text,
        title -> Nullable<Text>,
        founded_at -> BigInt,
        founded_by -> Text,
        mayor -> BigInt,
        folder -> Nullable<Text>,
        mode -> Text,
        tier -> BigInt,
        site_x -> Nullable<Double>,
        site_z -> Nullable<Double>,
        next_number -> BigInt,
        limits -> Text,
        removed_at -> Nullable<BigInt>,
        removed_by -> Nullable<Text>,
        removed_reason -> Nullable<Text>,
        erased -> Bool,
    }
}

diesel::table! {
    hamlet_chief (hamlet, chief) {
        hamlet -> Text,
        chief -> BigInt,
    }
}

diesel::table! {
    hamlet_folder (hamlet, folder) {
        hamlet -> Text,
        folder -> Text,
        at -> BigInt,
    }
}

diesel::table! {
    hand (agent) {
        agent -> BigInt,
        scholar -> BigInt,
        model -> Nullable<Text>,
    }
}

diesel::table! {
    hire (agent) {
        agent -> BigInt,
        hamlet -> Text,
        number -> BigInt,
        scholar -> BigInt,
        parent -> Nullable<BigInt>,
        purpose -> Text,
        nickname -> Nullable<Text>,
        pin -> Nullable<Text>,
        base -> Nullable<Text>,
        model -> Nullable<Text>,
        model_by -> Nullable<Text>,
        model_reason -> Nullable<Text>,
        allowance -> Nullable<BigInt>,
        allowance_by -> Nullable<Text>,
        merged_into -> Nullable<BigInt>,
    }
}

diesel::table! {
    hire_merge (id) {
        id -> BigInt,
        kept -> BigInt,
        absorbed -> BigInt,
        at -> BigInt,
        by -> Text,
        reason -> Text,
        merge_commit -> Nullable<Text>,
        clashes -> Nullable<Text>,
        split_at -> Nullable<BigInt>,
        split_by -> Nullable<Text>,
        split_commit -> Nullable<Text>,
    }
}

diesel::table! {
    hire_tool_off (hire, tool) {
        hire -> BigInt,
        tool -> Text,
        at -> BigInt,
        by -> Text,
    }
}

diesel::table! {
    input (conversation, request) {
        conversation -> BigInt,
        request -> Text,
        ordinal -> BigInt,
        kind -> Text,
        task -> Nullable<BigInt>,
        content -> Text,
        mark -> Text,
        steer -> Bool,
        boundary -> Bool,
        state -> Text,
        submission -> Nullable<Text>,
        status -> Nullable<Text>,
        reason -> Nullable<Text>,
        detail -> Nullable<Text>,
        followed -> Bool,
    }
}

diesel::table! {
    item (conversation, entry) {
        conversation -> BigInt,
        entry -> Text,
        ordinal -> BigInt,
        body -> Text,
    }
}

diesel::table! {
    job (id) {
        id -> BigInt,
        kind -> Text,
        key -> Nullable<Text>,
        payload -> Text,
        made -> BigInt,
        after -> Nullable<BigInt>,
        tries -> BigInt,
        failure -> Nullable<Text>,
        done -> Nullable<BigInt>,
    }
}

diesel::table! {
    library_event (id) {
        id -> BigInt,
        kind -> Text,
        skill -> Text,
        agent -> Nullable<BigInt>,
        chief -> Nullable<BigInt>,
        at -> BigInt,
        reason -> Nullable<Text>,
        commit_id -> Nullable<Text>,
    }
}

diesel::table! {
    marker (source, key) {
        source -> Text,
        key -> Text,
        kind -> Text,
        digest -> Binary,
        outcome -> Text,
        at -> BigInt,
    }
}

diesel::table! {
    message (id) {
        id -> BigInt,
        kind -> Text,
        runtime -> Text,
        key -> Text,
        sender -> BigInt,
        receiver -> BigInt,
        from_conversation -> BigInt,
        to_conversation -> BigInt,
        address -> Text,
        task -> BigInt,
        text -> Text,
        at -> BigInt,
        input -> Nullable<Text>,
        answer -> Nullable<Text>,
        failure -> Nullable<Text>,
        carried -> Bool,
        replied -> Bool,
        refused -> Bool,
    }
}

diesel::table! {
    objection (task) {
        task -> BigInt,
        agent -> BigInt,
        source -> Text,
        reason -> Text,
        wait -> Nullable<BigInt>,
        at -> BigInt,
        ruling -> Nullable<Text>,
        ruled_at -> Nullable<BigInt>,
    }
}

diesel::table! {
    permission_grant (id) {
        id -> BigInt,
        agent -> BigInt,
        permission -> Text,
        at -> BigInt,
        chief -> BigInt,
        revoked_at -> Nullable<BigInt>,
        revoked_by -> Nullable<Text>,
    }
}

diesel::table! {
    realm (id) {
        id -> BigInt,
        identity -> Text,
        version -> Text,
        made -> BigInt,
    }
}

diesel::table! {
    reflection (conversation, request) {
        conversation -> BigInt,
        request -> Text,
        ordinal -> BigInt,
        count -> BigInt,
        task -> BigInt,
        at -> BigInt,
    }
}

diesel::table! {
    review (task) {
        task -> BigInt,
        lead -> BigInt,
        conversation -> Nullable<BigInt>,
        outcome -> Nullable<Text>,
        at -> BigInt,
    }
}

diesel::table! {
    road (id) {
        id -> BigInt,
        a -> BigInt,
        b -> BigInt,
        kind -> Text,
        laid_at -> BigInt,
        laid_by -> Text,
        reason -> Text,
        task -> Nullable<BigInt>,
        inherited_from -> Nullable<BigInt>,
        up_at -> Nullable<BigInt>,
        up_by -> Nullable<Text>,
        up_reason -> Nullable<Text>,
    }
}

diesel::table! {
    road_charge (id) {
        id -> BigInt,
        road -> BigInt,
        hamlet -> Text,
        agent -> Nullable<BigInt>,
        task -> Nullable<BigInt>,
        at -> BigInt,
        micros -> BigInt,
    }
}

diesel::table! {
    runtime (id) {
        id -> Text,
        harness -> Text,
        adapter -> Nullable<Text>,
        version -> Nullable<Text>,
        begun -> Bool,
    }
}

diesel::table! {
    scholar (id) {
        id -> BigInt,
        kind -> Text,
        name -> Text,
        chief -> Nullable<BigInt>,
        since -> BigInt,
        made_by -> Text,
        planted -> Bool,
        arrival -> Text,
        arrival_by -> Nullable<Text>,
        house_x -> Nullable<Double>,
        house_z -> Nullable<Double>,
        house_turn -> Nullable<Double>,
        fork_of -> Nullable<BigInt>,
        fork_at -> Nullable<Text>,
        left_at -> Nullable<BigInt>,
        left_by -> Nullable<Text>,
        left_reason -> Nullable<Text>,
        erased -> Bool,
    }
}

diesel::table! {
    skill_offer (id) {
        id -> BigInt,
        agent -> BigInt,
        skill -> Text,
        reason -> Text,
        task -> Nullable<BigInt>,
        runtime -> Text,
        key -> Text,
        tree -> Nullable<Text>,
        verdict -> Nullable<Text>,
        words -> Nullable<Text>,
        at -> BigInt,
    }
}

diesel::table! {
    task (id) {
        id -> BigInt,
        origin -> Text,
        agent -> BigInt,
        chief -> Nullable<BigInt>,
        payer -> Nullable<Text>,
        at -> BigInt,
        text -> Nullable<Text>,
        messages -> BigInt,
        done_at -> Nullable<BigInt>,
    }
}

diesel::table! {
    tool_call (runtime, key) {
        runtime -> Text,
        key -> Text,
        conversation -> BigInt,
        digest -> Binary,
        input -> Nullable<Text>,
        tool -> Text,
        net -> Nullable<Bool>,
        block -> Nullable<Text>,
        wait -> Nullable<BigInt>,
        how -> Nullable<Text>,
        text -> Nullable<Text>,
        change -> Nullable<Text>,
    }
}

diesel::table! {
    trust (agent, chief, session) {
        agent -> BigInt,
        chief -> BigInt,
        session -> BigInt,
        edit -> Bool,
        command -> Bool,
    }
}

diesel::table! {
    turn_gate (conversation) {
        conversation -> BigInt,
        input -> Nullable<Text>,
        strikes -> BigInt,
        questions -> BigInt,
        refused -> Text,
    }
}

diesel::table! {
    usage (runtime, key) {
        runtime -> Text,
        key -> Text,
        ordinal -> BigInt,
        input -> BigInt,
        output -> BigInt,
        cache_read -> BigInt,
        cache_write -> BigInt,
        micros -> BigInt,
        ms -> BigInt,
        estimated -> Bool,
        detail -> Text,
        at -> BigInt,
    }
}

diesel::table! {
    wait (id) {
        id -> BigInt,
        kind -> Text,
        danger -> Nullable<Text>,
        agent -> Nullable<BigInt>,
        scholar -> Nullable<BigInt>,
        conversation -> Nullable<BigInt>,
        task -> Nullable<BigInt>,
        about -> Text,
        change -> Nullable<Text>,
        answers -> Text,
        questions -> Nullable<Text>,
        since -> BigInt,
        asked -> BigInt,
        open -> Bool,
        how -> Nullable<Text>,
        text -> Nullable<Text>,
        chief -> Nullable<BigInt>,
        settled -> Nullable<BigInt>,
    }
}

diesel::table! {
    worker (agent) {
        agent -> BigInt,
        starter -> Nullable<BigInt>,
        chief -> Nullable<BigInt>,
        hamlet -> Nullable<Text>,
        brief -> Text,
        tools -> Text,
        model -> Nullable<Text>,
        waits -> Bool,
        call_runtime -> Nullable<Text>,
        call_key -> Nullable<Text>,
        answer -> Nullable<Text>,
        nickname -> Nullable<Text>,
    }
}

diesel::joinable!(agreement -> hamlet (hamlet));
diesel::joinable!(agreement -> road (road));
diesel::joinable!(agreement -> task (task));
diesel::joinable!(answer_fault -> conversation (conversation));
diesel::joinable!(attempt -> agent (agent));
diesel::joinable!(attempt -> conversation (conversation));
diesel::joinable!(attempt -> hamlet (payer));
diesel::joinable!(attempt -> task (task));
diesel::joinable!(budget_reset -> chief (chief));
diesel::joinable!(budget_reset -> hamlet (hamlet));
diesel::joinable!(chat -> agent (agent));
diesel::joinable!(chat -> chief (chief));
diesel::joinable!(chat_session -> conversation (conversation));
diesel::joinable!(conversation -> agent (agent));
diesel::joinable!(conversation -> chief (chief));
diesel::joinable!(conversation -> runtime (runtime));
diesel::joinable!(decision -> hamlet (hamlet));
diesel::joinable!(decision -> scholar (scholar));
diesel::joinable!(decision -> task (task));
diesel::joinable!(document -> chief (chief));
diesel::joinable!(dream -> conversation (conversation));
diesel::joinable!(dream -> hire (hire));
diesel::joinable!(dream -> scholar (scholar));
diesel::joinable!(dream -> task (task));
diesel::joinable!(hamlet_chief -> chief (chief));
diesel::joinable!(hamlet_chief -> hamlet (hamlet));
diesel::joinable!(hamlet_folder -> hamlet (hamlet));
diesel::joinable!(hand -> agent (agent));
diesel::joinable!(hand -> scholar (scholar));
diesel::joinable!(hire -> agent (agent));
diesel::joinable!(hire -> hamlet (hamlet));
diesel::joinable!(hire -> scholar (scholar));
diesel::joinable!(hire_tool_off -> hire (hire));
diesel::joinable!(input -> conversation (conversation));
diesel::joinable!(input -> task (task));
diesel::joinable!(item -> conversation (conversation));
diesel::joinable!(library_event -> agent (agent));
diesel::joinable!(library_event -> chief (chief));
diesel::joinable!(message -> task (task));
diesel::joinable!(objection -> agent (agent));
diesel::joinable!(objection -> task (task));
diesel::joinable!(objection -> wait (wait));
diesel::joinable!(permission_grant -> agent (agent));
diesel::joinable!(permission_grant -> chief (chief));
diesel::joinable!(reflection -> conversation (conversation));
diesel::joinable!(reflection -> task (task));
diesel::joinable!(review -> agent (lead));
diesel::joinable!(review -> conversation (conversation));
diesel::joinable!(review -> task (task));
diesel::joinable!(road -> task (task));
diesel::joinable!(road_charge -> agent (agent));
diesel::joinable!(road_charge -> hamlet (hamlet));
diesel::joinable!(road_charge -> road (road));
diesel::joinable!(road_charge -> task (task));
diesel::joinable!(scholar -> chief (chief));
diesel::joinable!(skill_offer -> agent (agent));
diesel::joinable!(skill_offer -> task (task));
diesel::joinable!(task -> agent (agent));
diesel::joinable!(task -> chief (chief));
diesel::joinable!(task -> hamlet (payer));
diesel::joinable!(tool_call -> conversation (conversation));
diesel::joinable!(tool_call -> wait (wait));
diesel::joinable!(trust -> agent (agent));
diesel::joinable!(trust -> chief (chief));
diesel::joinable!(turn_gate -> conversation (conversation));
diesel::joinable!(wait -> agent (agent));
diesel::joinable!(wait -> chief (chief));
diesel::joinable!(wait -> conversation (conversation));
diesel::joinable!(wait -> scholar (scholar));
diesel::joinable!(wait -> task (task));
diesel::joinable!(worker -> agent (agent));
diesel::joinable!(worker -> chief (chief));
diesel::joinable!(worker -> hamlet (hamlet));

diesel::allow_tables_to_appear_in_same_query!(
    agent,
    agreement,
    answer_fault,
    attempt,
    budget_reset,
    chat,
    chat_session,
    chief,
    conversation,
    decision,
    document,
    dream,
    hamlet,
    hamlet_chief,
    hamlet_folder,
    hand,
    hire,
    hire_merge,
    hire_tool_off,
    input,
    item,
    job,
    library_event,
    marker,
    message,
    objection,
    permission_grant,
    realm,
    reflection,
    review,
    road,
    road_charge,
    runtime,
    scholar,
    skill_offer,
    task,
    tool_call,
    trust,
    turn_gate,
    usage,
    wait,
    worker,
);

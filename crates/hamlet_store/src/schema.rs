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
    agent_merge (id) {
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
    agent_tool_off (agent, tool) {
        agent -> BigInt,
        tool -> Text,
        at -> BigInt,
        by -> Text,
    }
}

diesel::table! {
    agreement (id) {
        id -> BigInt,
        a -> BigInt,
        b -> BigInt,
        hamlet -> Text,
        lead -> BigInt,
        at -> BigInt,
        reason -> Text,
        task -> Nullable<BigInt>,
        channel -> Nullable<BigInt>,
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
    asked_dream (id) {
        id -> BigInt,
        profile -> Nullable<BigInt>,
        agent -> Nullable<BigInt>,
        at -> BigInt,
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
        channel_charge -> BigInt,
    }
}

diesel::table! {
    channel (id) {
        id -> BigInt,
        a -> BigInt,
        b -> BigInt,
        kind -> Text,
        opened_at -> BigInt,
        opened_by -> Text,
        reason -> Text,
        task -> Nullable<BigInt>,
        inherited_from -> Nullable<BigInt>,
        closed_at -> Nullable<BigInt>,
        closed_by -> Nullable<Text>,
        closed_reason -> Nullable<Text>,
    }
}

diesel::table! {
    channel_charge (id) {
        id -> BigInt,
        channel -> BigInt,
        hamlet -> Text,
        agent -> Nullable<BigInt>,
        task -> Nullable<BigInt>,
        at -> BigInt,
        micros -> BigInt,
    }
}

diesel::table! {
    chat (agent) {
        agent -> BigInt,
        conversation -> BigInt,
        reflections -> BigInt,
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
        held -> Bool,
        runs_first -> Nullable<Text>,
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
        profile -> Nullable<BigInt>,
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
        profile -> BigInt,
        agent -> Nullable<BigInt>,
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
        name -> Text,
        founded_at -> BigInt,
        founded_by -> Text,
        lead -> BigInt,
        folder -> Nullable<Text>,
        mode -> Text,
        tier -> BigInt,
        site_x -> Nullable<Double>,
        site_z -> Nullable<Double>,
        limits -> Text,
        removed_at -> Nullable<BigInt>,
        removed_by -> Nullable<Text>,
        removed_reason -> Nullable<Text>,
        erased -> Bool,
    }
}

diesel::table! {
    hamlet_agent (agent) {
        agent -> BigInt,
        hamlet -> Text,
        profile -> BigInt,
        parent -> Nullable<BigInt>,
        purpose -> Text,
        name -> Nullable<Text>,
        pin -> Nullable<Text>,
        base -> Nullable<Text>,
        model -> Nullable<Text>,
        model_by -> Nullable<Text>,
        model_reason -> Nullable<Text>,
        allowance -> Nullable<BigInt>,
        allowance_by -> Nullable<Text>,
        merged_into -> Nullable<BigInt>,
        ending_dream -> Bool,
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
        profile -> BigInt,
        model -> Nullable<Text>,
    }
}

diesel::table! {
    import (conversation) {
        conversation -> BigInt,
        agent -> BigInt,
        chief -> BigInt,
        source -> Text,
        session -> Text,
        earlier -> Nullable<Text>,
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
        response -> Nullable<Text>,
        failure -> Nullable<Text>,
        carried -> Bool,
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
    profile (id) {
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
        parent -> BigInt,
        conversation -> Nullable<BigInt>,
        outcome -> Nullable<Text>,
        at -> BigInt,
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
    session_grant (conversation, chief) {
        conversation -> BigInt,
        chief -> BigInt,
        edit -> Bool,
        command -> Bool,
    }
}

diesel::table! {
    session_tool_grant (conversation, chief, tool) {
        conversation -> BigInt,
        chief -> BigInt,
        tool -> Text,
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
        profile -> Nullable<BigInt>,
        conversation -> Nullable<BigInt>,
        task -> Nullable<BigInt>,
        about -> Text,
        change -> Nullable<Text>,
        answers -> Text,
        questions -> Nullable<Text>,
        setup -> Nullable<Text>,
        server -> Nullable<Text>,
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
        name -> Nullable<Text>,
    }
}

diesel::joinable!(agent_tool_off -> hamlet_agent (agent));
diesel::joinable!(agreement -> channel (channel));
diesel::joinable!(agreement -> hamlet (hamlet));
diesel::joinable!(agreement -> task (task));
diesel::joinable!(answer_fault -> conversation (conversation));
diesel::joinable!(asked_dream -> hamlet_agent (agent));
diesel::joinable!(asked_dream -> profile (profile));
diesel::joinable!(attempt -> agent (agent));
diesel::joinable!(attempt -> conversation (conversation));
diesel::joinable!(attempt -> hamlet (payer));
diesel::joinable!(attempt -> task (task));
diesel::joinable!(budget_reset -> chief (chief));
diesel::joinable!(budget_reset -> hamlet (hamlet));
diesel::joinable!(channel -> task (task));
diesel::joinable!(channel_charge -> agent (agent));
diesel::joinable!(channel_charge -> channel (channel));
diesel::joinable!(channel_charge -> hamlet (hamlet));
diesel::joinable!(channel_charge -> task (task));
diesel::joinable!(chat -> agent (agent));
diesel::joinable!(chat -> conversation (conversation));
diesel::joinable!(conversation -> agent (agent));
diesel::joinable!(conversation -> runtime (runtime));
diesel::joinable!(decision -> hamlet (hamlet));
diesel::joinable!(decision -> profile (profile));
diesel::joinable!(decision -> task (task));
diesel::joinable!(document -> chief (chief));
diesel::joinable!(dream -> conversation (conversation));
diesel::joinable!(dream -> hamlet_agent (agent));
diesel::joinable!(dream -> profile (profile));
diesel::joinable!(dream -> task (task));
diesel::joinable!(hamlet_agent -> agent (agent));
diesel::joinable!(hamlet_agent -> hamlet (hamlet));
diesel::joinable!(hamlet_agent -> profile (profile));
diesel::joinable!(hamlet_chief -> chief (chief));
diesel::joinable!(hamlet_chief -> hamlet (hamlet));
diesel::joinable!(hamlet_folder -> hamlet (hamlet));
diesel::joinable!(hand -> agent (agent));
diesel::joinable!(hand -> profile (profile));
diesel::joinable!(import -> agent (agent));
diesel::joinable!(import -> chief (chief));
diesel::joinable!(import -> conversation (conversation));
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
diesel::joinable!(profile -> chief (chief));
diesel::joinable!(reflection -> conversation (conversation));
diesel::joinable!(reflection -> task (task));
diesel::joinable!(review -> agent (parent));
diesel::joinable!(review -> conversation (conversation));
diesel::joinable!(review -> task (task));
diesel::joinable!(session_grant -> chief (chief));
diesel::joinable!(session_grant -> conversation (conversation));
diesel::joinable!(session_tool_grant -> chief (chief));
diesel::joinable!(session_tool_grant -> conversation (conversation));
diesel::joinable!(skill_offer -> agent (agent));
diesel::joinable!(skill_offer -> task (task));
diesel::joinable!(task -> agent (agent));
diesel::joinable!(task -> chief (chief));
diesel::joinable!(task -> hamlet (payer));
diesel::joinable!(tool_call -> conversation (conversation));
diesel::joinable!(tool_call -> wait (wait));
diesel::joinable!(turn_gate -> conversation (conversation));
diesel::joinable!(wait -> agent (agent));
diesel::joinable!(wait -> chief (chief));
diesel::joinable!(wait -> conversation (conversation));
diesel::joinable!(wait -> profile (profile));
diesel::joinable!(wait -> task (task));
diesel::joinable!(worker -> agent (agent));
diesel::joinable!(worker -> chief (chief));
diesel::joinable!(worker -> hamlet (hamlet));

diesel::allow_tables_to_appear_in_same_query!(
    agent,
    agent_merge,
    agent_tool_off,
    agreement,
    answer_fault,
    asked_dream,
    attempt,
    budget_reset,
    channel,
    channel_charge,
    chat,
    chief,
    conversation,
    decision,
    document,
    dream,
    hamlet,
    hamlet_agent,
    hamlet_chief,
    hamlet_folder,
    hand,
    import,
    input,
    item,
    job,
    library_event,
    marker,
    message,
    objection,
    permission_grant,
    profile,
    realm,
    reflection,
    review,
    runtime,
    session_grant,
    session_tool_grant,
    skill_offer,
    task,
    tool_call,
    turn_gate,
    usage,
    wait,
    worker,
);

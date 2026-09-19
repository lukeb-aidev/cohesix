// Author: Lukas Bower
// Purpose: Own a loopback OpenDNP3 reference with known static values and acknowledged CROBs, without simulating physical plant behavior.
// Copyright 2026 Lukas Bower
#include <opendnp3/DNP3Manager.h>
#include <opendnp3/ConsoleLogger.h>
#include <opendnp3/channel/PrintingChannelListener.h>
#include <opendnp3/outstation/DefaultOutstationApplication.h>
#include <opendnp3/outstation/SimpleCommandHandler.h>
#include <opendnp3/outstation/UpdateBuilder.h>
#include <charconv>
#include <chrono>
#include <iostream>
#include <string>
#include <string_view>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    const std::string_view argument(argv[1]);
    unsigned port = 0;
    const auto parsed = std::from_chars(argument.data(), argument.data() + argument.size(), port);
    if (parsed.ec != std::errc{} || parsed.ptr != argument.data() + argument.size()
        || port < 1024 || port > 65535) return 2;
    using namespace opendnp3;
    DNP3Manager manager(1, ConsoleLogger::Create());
    auto channel = manager.AddTCPServer("reference", levels::NORMAL | levels::ALL_COMMS,
        ServerAcceptMode::CloseExisting, IPEndpoint("127.0.0.1", static_cast<uint16_t>(port)), PrintingChannelListener::Create());
    DatabaseConfig database(4);
    database.analog_input[0].svariation = StaticAnalogVariation::Group30Var1;
    OutstationStackConfig configuration(database);
    configuration.link.LocalAddr = 10;
    configuration.link.RemoteAddr = 1;
    configuration.link.KeepAliveTimeout = TimeDuration::Max();
    configuration.outstation.params.allowUnsolicited = false;
    configuration.outstation.eventBufferConfig = EventBufferConfig::AllTypes(4);
    auto application = std::make_shared<DefaultOutstationApplication>(TimeDuration::Minutes(60));
    application->WriteAbsoluteTime(UTCTimestamp(static_cast<uint64_t>(std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::system_clock::now().time_since_epoch()).count())));
    auto outstation = channel->AddOutstation("reference", SuccessCommandHandler::Create(), application, configuration);
    UpdateBuilder update;
    update.Update(Analog(42, Flags(0x01)), 0);
    update.Update(Binary(true, Flags(0x01)), 0);
    update.Update(Counter(7, Flags(0x01)), 0);
    outstation->Apply(update.Build());
    outstation->Enable();
    std::cout << "READY" << std::endl;
    std::string line;
    while (std::getline(std::cin, line)) {
        if (line == "quit") break;
        if (line.size() > 128) return 2;
    }
    return 0;
}
